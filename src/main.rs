use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use enlightenviz::board::schema_json;
use enlightenviz::catalog::load_dir;
use enlightenviz::db::{Database, Settings};
use enlightenviz::diagnostic::{Diagnostic, to_json};
use enlightenviz::log;
use enlightenviz::server::{AppState, PrepareError, serve};

const URL_VARIABLE: &str = "ENLIGHTENVIZ_DATABASE_URL";

#[derive(Parser)]
#[command(
    name = "enlightenviz",
    version,
    about = "A small server that draws dashboards written as code."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the JSON Schema of a board.
    Schema,
    /// Check every board in a directory, and print what to change.
    ///
    /// Offline it checks the YAML, the schema, the SQL parameters and which
    /// query each chart reads. If a database is configured it also prepares
    /// every query in a read-only transaction and checks that every field a
    /// chart draws is a column the query returns. The exit code is 0 when
    /// nothing is wrong, 1 when a board has a problem, and 2 when the run
    /// itself failed.
    Validate(ValidateArgs),
    /// Check the boards, then serve them over HTTP.
    Serve(ServeArgs),
}

#[derive(Args)]
struct ConnectionArgs {
    /// A file that holds the Postgres connection URL. Alternatively set the
    /// environment variable ENLIGHTENVIZ_DATABASE_URL; there is deliberately
    /// no flag for the URL itself, because arguments are visible to every
    /// user of the machine.
    #[arg(long, env = "ENLIGHTENVIZ_DATABASE_URL_FILE", value_name = "PATH")]
    database_url_file: Option<PathBuf>,
    /// Accept that the connection to Postgres is not encrypted. This build
    /// has no TLS to Postgres, and refuses to connect unless told this is
    /// meant.
    #[arg(long, env = "ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE")]
    allow_unencrypted_database: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Args)]
struct ValidateArgs {
    /// The directory that holds the board files (.yml, .yaml).
    dir: PathBuf,
    /// How to print the result.
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
    #[command(flatten)]
    connection: ConnectionArgs,
}

#[derive(Args)]
struct ServeArgs {
    /// The directory that holds the board files (.yml, .yaml).
    #[arg(long, env = "ENLIGHTENVIZ_BOARDS", value_name = "DIR")]
    boards: PathBuf,
    /// The address to listen on.
    #[arg(long, env = "ENLIGHTENVIZ_LISTEN", default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    /// The most connections to Postgres.
    #[arg(long, env = "ENLIGHTENVIZ_POOL_SIZE", default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=64))]
    pool_size: u16,
    /// How long a request waits for a free connection before it gets a 503.
    #[arg(long, env = "ENLIGHTENVIZ_ACQUIRE_TIMEOUT_MS", default_value_t = 2000, value_parser = clap::value_parser!(u64).range(1..=60_000))]
    acquire_timeout_ms: u64,
    /// How long one statement may run before Postgres cancels it.
    #[arg(long, env = "ENLIGHTENVIZ_STATEMENT_TIMEOUT_MS", default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=600_000))]
    statement_timeout_ms: u64,
    /// The most rows a query may return; a result over it is an error.
    #[arg(long, env = "ENLIGHTENVIZ_ROW_LIMIT", default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
    row_limit: u32,
    #[command(flatten)]
    connection: ConnectionArgs,
}

/// A failure of the run itself (not a problem in a board): exit code 2.
type Failure = String;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Schema => schema(),
        Command::Validate(args) => validate(args).await,
        Command::Serve(args) => run_server(args).await,
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn schema() -> Result<ExitCode, Failure> {
    let text = schema_json().map_err(|e| format!("the schema cannot be written: {e}"))?;
    print!("{text}");
    Ok(ExitCode::SUCCESS)
}

/// The connection URL, if one is configured: from the environment variable or
/// from a file, never both.
fn database_url(args: &ConnectionArgs) -> Result<Option<String>, Failure> {
    let from_env = match std::env::var(URL_VARIABLE) {
        Ok(url) => Some(url),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!("{URL_VARIABLE} is not valid UTF-8"));
        }
    };
    let from_file = match &args.database_url_file {
        Some(path) => Some(std::fs::read_to_string(path).map_err(|e| {
            format!(
                "the database URL file {} cannot be read: {e}",
                path.display()
            )
        })?),
        None => None,
    };
    let url = match (from_env, from_file) {
        (Some(_), Some(_)) => {
            return Err(format!(
                "both {URL_VARIABLE} and a database URL file are set; use one"
            ));
        }
        (None, None) => return Ok(None),
        (Some(url), None) | (None, Some(url)) => url.trim().to_string(),
    };
    if url.is_empty() {
        return Err("the database URL is empty".to_string());
    }
    if !args.allow_unencrypted_database {
        return Err(
            "this build connects to Postgres without TLS, so the traffic, including the \
            password, is not encrypted. Use it only on a network you trust, and say so with \
            --allow-unencrypted-database (ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE=true)"
                .to_string(),
        );
    }
    Ok(Some(url))
}

fn print_text(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        println!("{d}");
    }
}

async fn validate(args: ValidateArgs) -> Result<ExitCode, Failure> {
    let (catalog, mut diagnostics) = load_dir(&args.dir);
    let url = database_url(&args.connection)?;
    let database_checked = url.is_some();
    if let Some(url) = url {
        let settings = Settings {
            url,
            pool_size: 1,
            acquire_timeout: Duration::from_secs(5),
            statement_timeout: Duration::from_secs(5),
            row_limit: 10_000,
        };
        diagnostics.extend(Database::new(&settings)?.check(&catalog).await);
    }
    diagnostics.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));

    match args.format {
        Format::Json => println!(
            "{}",
            to_json(&diagnostics, database_checked)
                .map_err(|e| format!("the report cannot be written: {e}"))?
        ),
        Format::Text => {
            print_text(&diagnostics);
            if !database_checked {
                println!(
                    "note: no database is configured, so SQL was not prepared and chart fields were not checked against columns"
                );
            }
            match diagnostics.len() {
                0 => println!("ok: {} board(s) valid", catalog.boards.len()),
                n => println!("{n} error(s)"),
            }
        }
    }
    Ok(if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

async fn run_server(args: ServeArgs) -> Result<ExitCode, Failure> {
    let url = database_url(&args.connection)?.ok_or_else(|| {
        format!("no database is configured: set {URL_VARIABLE}, or give --database-url-file")
    })?;
    let row_limit = args.row_limit as usize;
    let settings = Settings {
        url,
        pool_size: usize::from(args.pool_size),
        acquire_timeout: Duration::from_millis(args.acquire_timeout_ms),
        statement_timeout: Duration::from_millis(args.statement_timeout_ms),
        row_limit,
    };

    let state = match AppState::prepare(&args.boards, &settings).await {
        Ok(state) => state,
        Err(PrepareError::Invalid(diagnostics)) => {
            for d in &diagnostics {
                eprintln!("{d}");
            }
            eprintln!("not starting: {} error(s)", diagnostics.len());
            return Ok(ExitCode::from(1));
        }
        Err(PrepareError::Failed(message)) => return Err(message),
    };

    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", args.listen))?;
    let (boards, queries) = state.counts();
    log::info(&format!(
        "listening on {} with {boards} board(s), {queries} query(ies), a pool of {}",
        args.listen, args.pool_size
    ));
    let state = Arc::new(state);
    serve(listener, state)
        .await
        .map_err(|e| format!("the server stopped: {e}"))?;
    Ok(ExitCode::SUCCESS)
}
