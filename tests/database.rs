//! Everything that needs a Postgres: validation against a database, and the
//! HTTP server's answers and safety limits.
//!
//! Set ENLIGHTENVIZ_TEST_DATABASE_URL to the URL of a superuser of a database
//! that may be dropped into: the first test creates the example's table, a
//! scratch table and three throwaway roles there. The tests fail, not skip,
//! when it is not set.

// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use enlightenviz::catalog::load_dir;
use enlightenviz::db::{Database, Settings};
use enlightenviz::server::{AppState, PrepareError, router};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls, config::Host};

// ---------------------------------------------------------------- harness

const API: &str = "tests/fixtures/api";
const READER: (&str, &str) = ("enlightenviz_reader", "reader-test-only");
const LOAD: (&str, &str) = ("enlightenviz_load", "load-test-only");
const DISCONNECT: (&str, &str) = ("enlightenviz_disconnect", "disconnect-test-only");
const WRITER: (&str, &str) = ("enlightenviz_writer", "writer-test-only");

fn admin_url() -> String {
    std::env::var("ENLIGHTENVIZ_TEST_DATABASE_URL").expect(
        "set ENLIGHTENVIZ_TEST_DATABASE_URL to a disposable database (see tests/database.rs)",
    )
}

/// The test database's address with another role's credentials.
fn url_for((role, password): (&str, &str)) -> String {
    let config: tokio_postgres::Config = admin_url().parse().unwrap();
    let Some(Host::Tcp(host)) = config.get_hosts().first() else {
        panic!("the test URL needs a TCP host")
    };
    let port = config.get_ports().first().copied().unwrap_or(5432);
    let db = config.get_dbname().unwrap_or("postgres");
    format!("postgres://{role}:{password}@{host}:{port}/{db}")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await.unwrap();
    tokio::spawn(connection);
    client
}

/// A superuser connection, after the schema and roles exist.
async fn admin() -> Client {
    static SETUP: OnceCell<()> = OnceCell::const_new();
    SETUP
        .get_or_init(|| async {
            let client = connect(&admin_url()).await;
            client
                .batch_execute(include_str!("../examples/earthquakes/schema.sql"))
                .await
                .unwrap();
            client
                .batch_execute(include_str!("fixtures/setup.sql"))
                .await
                .unwrap();
        })
        .await;
    connect(&admin_url()).await
}

struct Options {
    role: (&'static str, &'static str),
    pool_size: usize,
    acquire_ms: u64,
    statement_ms: u64,
    row_limit: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            role: READER,
            pool_size: 4,
            acquire_ms: 2000,
            statement_ms: 5000,
            row_limit: 10_000,
        }
    }
}

fn settings(options: &Options) -> Settings {
    Settings {
        url: url_for(options.role),
        pool_size: options.pool_size,
        acquire_timeout: Duration::from_millis(options.acquire_ms),
        statement_timeout: Duration::from_millis(options.statement_ms),
        row_limit: options.row_limit,
    }
}

struct Server {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start(dir: &str, options: Options) -> Server {
    admin().await;
    let state = match AppState::prepare(Path::new(dir), &settings(&options)).await {
        Ok(state) => state,
        Err(e) => panic!("the server cannot start: {e:?}"),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::new(state));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { addr, task }
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }

    fn error_code(&self) -> String {
        self.json()["error"]["code"].as_str().unwrap().to_string()
    }
}

/// One HTTP/1.1 GET over a fresh connection.
async fn get(addr: SocketAddr, target: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {target} HTTP/1.1\r\nhost: test\r\nconnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    Reply {
        status,
        head: head.to_lowercase(),
        body: body.to_string(),
    }
}

fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn query_url(name: &str, query: &str) -> String {
    format!("/api/boards/api/queries/{name}?{query}")
}

/// Nothing that identifies the SQL, the schema or the database may reach a
/// client.
fn assert_no_leak(reply: &Reply) {
    let body = reply.body.to_lowercase();
    for word in [
        "select",
        "earthquakes_daily",
        "scratch",
        "postgres",
        "password",
        "sqlstate",
        "read-only",
        "pg_sleep",
    ] {
        assert!(
            !body.contains(word),
            "the answer leaks `{word}`: {}",
            reply.body
        );
    }
    let error = &reply.json()["error"];
    assert!(!error["id"].as_str().unwrap().is_empty());
}

// ------------------------------------------------------------- validation

#[tokio::test]
async fn validation_against_a_database_finds_what_offline_cannot() {
    admin().await;
    let (catalog, offline) = load_dir(Path::new("tests/fixtures/db_invalid"));
    assert!(offline.is_empty(), "{offline:?}");
    let db = Database::new(&settings(&Options::default())).unwrap();
    let mut found: Vec<_> = db
        .check(&catalog)
        .await
        .into_iter()
        .map(|d| {
            (
                Path::new(&d.file)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                d.line,
                d.rule,
            )
        })
        .collect();
    found.sort();
    let expected = vec![
        (
            "chart_fields.yml".to_string(),
            Some(22),
            "chart-unknown-field",
        ),
        ("columns.yml".to_string(), Some(4), "filter-unknown-column"),
        ("columns.yml".to_string(), Some(7), "stat-unknown-column"),
        ("columns.yml".to_string(), Some(18), "chart-unknown-field"),
        ("columns.yml".to_string(), Some(19), "chart-unknown-field"),
        ("columns.yml".to_string(), Some(21), "chart-unknown-field"),
        (
            "duplicate_column.yml".to_string(),
            Some(5),
            "query-column-duplicate",
        ),
        (
            "prepare_fails.yml".to_string(),
            Some(5),
            "sql-prepare-failed",
        ),
        (
            "prepare_fails.yml".to_string(),
            Some(7),
            "sql-prepare-failed",
        ),
        (
            "prepare_fails.yml".to_string(),
            Some(11),
            "sql-not-a-select",
        ),
        (
            "prepare_fails.yml".to_string(),
            Some(13),
            "sql-not-a-select",
        ),
    ];
    assert_eq!(found, expected);
}

#[tokio::test]
async fn the_good_boards_pass_against_a_database() {
    admin().await;
    for dir in ["examples/earthquakes", API, "tests/fixtures/writes"] {
        let (catalog, offline) = load_dir(Path::new(dir));
        assert!(offline.is_empty(), "{dir}: {offline:?}");
        let db = Database::new(&settings(&Options::default())).unwrap();
        let found = db.check(&catalog).await;
        assert!(found.is_empty(), "{dir}: {found:?}");
    }
}

#[tokio::test]
async fn an_unreachable_database_is_one_diagnostic() {
    let mut options = settings(&Options::default());
    options.url = "postgres://nobody:nothing@127.0.0.1:1/none".to_string();
    match AppState::prepare(Path::new(API), &options).await {
        Err(PrepareError::Invalid(found)) => {
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].rule, "database-unreachable");
            assert!(
                !found[0].message.contains("nothing"),
                "the password is in the message"
            );
        }
        other => panic!("expected a diagnostic, got {:?}", other.err()),
    }
}

#[tokio::test]
async fn the_server_does_not_start_on_a_bad_board() {
    admin().await;
    match AppState::prepare(
        Path::new("tests/fixtures/db_invalid"),
        &settings(&Options::default()),
    )
    .await
    {
        Err(PrepareError::Invalid(found)) => assert_eq!(found.len(), 11),
        other => panic!("expected the start to be refused, got {:?}", other.err()),
    }
}

#[tokio::test]
async fn the_serve_command_stops_with_the_diagnostics_on_a_bad_board() {
    admin().await;
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_enlightenviz"))
        .args([
            "serve",
            "--boards",
            "tests/fixtures/db_invalid",
            "--allow-unencrypted-database",
        ])
        .env("ENLIGHTENVIZ_DATABASE_URL", url_for(READER))
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[chart-unknown-field]")
            && stderr.contains("not starting: 11 error(s)"),
        "{stderr}"
    );
    assert!(!stderr.contains("reader-test-only"), "{stderr}");
}

// ------------------------------------------------------------------ answers

#[tokio::test]
async fn a_query_answers_with_columns_and_rows_in_order() {
    let server = start(API, Options::default()).await;
    let reply = get(
        server.addr,
        &query_url("by_region", "region=Andes&from=2024-01-28"),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(
        reply.head.contains("content-type: application/json"),
        "{}",
        reply.head
    );
    assert!(
        reply.head.contains("cache-control: no-store"),
        "{}",
        reply.head
    );

    let expected = admin()
        .await
        .query(
            "select day::text, events from earthquakes_daily where region = 'Andes' and day >= date '2024-01-28' order by day",
            &[],
        )
        .await
        .unwrap();
    let rows: Vec<Value> = expected
        .iter()
        .map(|r| json!({"day": r.get::<_, String>(0), "events": r.get::<_, i32>(1)}))
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        reply.json(),
        json!({"columns": ["day", "events"], "rows": rows})
    );

    // The default of `from` applies when the request gives none: the whole month.
    let reply = get(server.addr, &query_url("by_region", "region=Andes")).await;
    assert_eq!(reply.json()["rows"].as_array().unwrap().len(), 30);
}

#[tokio::test]
async fn every_type_round_trips_through_text_and_a_cast() {
    let server = start(API, Options::default()).await;
    let query = format!(
        "text={}&number=1.5&flag=true&date=2024-02-29&items=1&items=2&items=3",
        enc("it's \"quoted\", é, a b")
    );
    let reply = get(server.addr, &query_url("echo", &query)).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        reply.json(),
        json!({
            "columns": ["text", "number", "flag", "date", "items"],
            "rows": [{"text": "it's \"quoted\", é, a b", "number": 1.5, "flag": true, "date": "2024-02-29", "items": [1, 2, 3]}]
        })
    );
    // `+` in a query string is a space.
    let reply = get(
        server.addr,
        &query_url(
            "echo",
            "text=a+b&number=2&flag=false&date=2024-01-01&items=7",
        ),
    )
    .await;
    assert_eq!(reply.json()["rows"][0]["text"], "a b");
    assert_eq!(reply.json()["rows"][0]["number"], 2);
}

#[tokio::test]
async fn a_column_named_like_the_wrapper_alias_still_works() {
    let server = start(API, Options::default()).await;
    let reply = get(
        server.addr,
        "/api/boards/api/queries/column_named_like_the_alias",
    )
    .await;
    assert_eq!(reply.json(), json!({"columns": ["q"], "rows": [{"q": 7}]}));
}

#[tokio::test]
async fn a_value_that_looks_like_sql_is_only_data() {
    let server = start(API, Options::default()).await;
    let attacks = [
        "x'; delete from earthquakes_daily; --",
        "' or '1'='1",
        "Andes' or 'a'='a",
        "$$; drop table earthquakes_daily; $$",
        "$1",
        "Andes\\' or 1=1 --",
        "\"; drop table earthquakes_daily; --",
    ];
    for attack in attacks {
        let reply = get(
            server.addr,
            &query_url("by_region", &format!("region={}", enc(attack))),
        )
        .await;
        assert_eq!(reply.status, 200, "{attack}: {}", reply.body);
        assert_eq!(reply.json()["rows"], json!([]), "{attack}");
    }
    let count: i64 = admin()
        .await
        .query_one("select count(*) from earthquakes_daily", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 120, "a row was deleted");
}

#[tokio::test]
async fn a_bad_request_is_a_precise_400_or_404_that_leaks_nothing() {
    let server = start(API, Options::default()).await;
    let echo = "text=a&number=1&flag=true&date=2024-01-01&items=1";
    let cases = [
        (
            "/api/boards/nope/queries/by_region?region=a".to_string(),
            404,
            "not_found",
        ),
        ("/api/boards/api/queries/nope".to_string(), 404, "not_found"),
        ("/api/nothing".to_string(), 404, "not_found"),
        (
            query_url("by_region", "region=Andes&bogus=1"),
            400,
            "bad_request",
        ),
        (query_url("by_region", ""), 400, "bad_request"),
        (
            query_url("by_region", "region=Andes&from=2024-13-01"),
            400,
            "bad_request",
        ),
        (
            query_url("by_region", "region=Andes&from=2024-01-01&from=2024-01-02"),
            400,
            "bad_request",
        ),
        (query_url("by_region", "region=%FF"), 400, "bad_request"),
        (query_url("by_region", "region=a%00b"), 400, "bad_request"),
        (
            query_url("echo", &echo.replace("number=1", "number=NaN")),
            400,
            "bad_request",
        ),
        (
            query_url("echo", &echo.replace("flag=true", "flag=maybe")),
            400,
            "bad_request",
        ),
        (
            query_url("echo", &format!("{echo}&items=2&items=3&items=4")),
            400,
            "bad_request",
        ),
        (
            query_url("echo", &echo.replace("items=1", "items=x")),
            400,
            "bad_request",
        ),
        (
            query_url("many", "count=9223372036854775808"),
            400,
            "bad_request",
        ),
    ];
    for (target, status, code) in cases {
        let reply = get(server.addr, &target).await;
        assert_eq!(
            (reply.status, reply.error_code().as_str()),
            (status, code),
            "{target}: {}",
            reply.body
        );
        assert_no_leak(&reply);
    }
    // The message names the parameter.
    let reply = get(server.addr, &query_url("by_region", "region=a&from=x")).await;
    assert!(
        reply.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("`from`"),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn healthz_answers_without_a_database_connection() {
    // The only connection is busy for a second: a query is refused, and
    // /healthz answers at once.
    let server = start(
        API,
        Options {
            pool_size: 1,
            acquire_ms: 200,
            ..Options::default()
        },
    )
    .await;
    let addr = server.addr;
    let busy = tokio::spawn(async move { get(addr, &query_url("slow", "seconds=1")).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let health = get(server.addr, "/healthz").await;
    assert_eq!((health.status, health.body.as_str()), (200, "ok\n"));
    let refused = get(server.addr, &query_url("by_region", "region=Andes")).await;
    assert_eq!(refused.status, 503);
    assert_eq!(busy.await.unwrap().status, 200);
}

// ------------------------------------------------------------------- safety

#[tokio::test]
async fn a_statement_that_writes_is_refused_by_the_read_only_transaction() {
    let setup = admin().await;
    // The role can write, as itself and in a plain transaction...
    let writer = connect(&url_for(WRITER)).await;
    assert_eq!(
        writer
            .query_one("select scratch_bump()", &[])
            .await
            .unwrap()
            .get::<_, i32>(0),
        1
    );
    // ...and a read-only one refuses it, which is what the server relies on.
    writer.batch_execute("begin read only").await.unwrap();
    let refused = writer
        .query_one("select scratch_bump()", &[])
        .await
        .unwrap_err();
    assert_eq!(refused.code().map(|c| c.code()), Some("25006"), "{refused}");
    drop(writer);
    setup
        .batch_execute("update scratch set n = 0")
        .await
        .unwrap();

    let server = start(
        "tests/fixtures/writes",
        Options {
            role: WRITER,
            ..Options::default()
        },
    )
    .await;
    for _ in 0..3 {
        let reply = get(server.addr, "/api/boards/writes/queries/bump").await;
        assert_eq!(
            (reply.status, reply.error_code().as_str()),
            (500, "internal"),
            "{}",
            reply.body
        );
        assert_no_leak(&reply);
    }
    let n: i32 = setup
        .query_one("select n from scratch", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 0, "the write went through");
}

#[tokio::test]
async fn a_slow_statement_is_cut_off_and_the_pool_still_works() {
    let server = start(
        API,
        Options {
            pool_size: 1,
            statement_ms: 300,
            ..Options::default()
        },
    )
    .await;
    let started = Instant::now();
    let reply = get(server.addr, &query_url("slow", "seconds=5")).await;
    assert_eq!(
        (reply.status, reply.error_code().as_str()),
        (504, "query_timeout"),
        "{}",
        reply.body
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the statement was not cancelled"
    );
    assert_no_leak(&reply);
    // The pool has one connection, so these use the one that was cancelled.
    for _ in 0..5 {
        let reply = get(server.addr, &query_url("by_region", "region=Andes")).await;
        assert_eq!(reply.status, 200, "{}", reply.body);
    }
    // A query that is quick enough still runs under the same limit.
    assert_eq!(
        get(server.addr, &query_url("slow", "seconds=0.05"))
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn a_result_over_the_row_limit_is_an_error_and_never_cut() {
    let server = start(
        API,
        Options {
            row_limit: 50,
            ..Options::default()
        },
    )
    .await;
    let exact = get(server.addr, &query_url("many", "count=50")).await;
    assert_eq!(exact.status, 200);
    assert_eq!(exact.json()["rows"].as_array().unwrap().len(), 50);
    let over = get(server.addr, &query_url("many", "count=51")).await;
    assert_eq!(
        (over.status, over.error_code().as_str()),
        (422, "row_limit"),
        "{}",
        over.body
    );
    assert!(!over.body.contains("\"rows\""), "{}", over.body);
    assert!(
        over.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("50")
    );
    let more = get(server.addr, &query_url("many", "count=1000")).await;
    assert_eq!(more.status, 422);
}

#[tokio::test]
async fn a_busy_pool_answers_503_and_recovers() {
    let server = start(
        API,
        Options {
            pool_size: 1,
            acquire_ms: 200,
            ..Options::default()
        },
    )
    .await;
    let addr = server.addr;
    let slow = tokio::spawn(async move { get(addr, &query_url("slow", "seconds=1")).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let refused = get(server.addr, &query_url("by_region", "region=Andes")).await;
    assert_eq!(
        (refused.status, refused.error_code().as_str()),
        (503, "busy"),
        "{}",
        refused.body
    );
    assert_no_leak(&refused);
    assert_eq!(slow.await.unwrap().status, 200);
    assert_eq!(
        get(server.addr, &query_url("by_region", "region=Andes"))
            .await
            .status,
        200
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_never_exceed_the_pool_under_load() {
    const POOL: usize = 4;
    let server = start(
        API,
        Options {
            role: LOAD,
            pool_size: POOL,
            acquire_ms: 30_000,
            ..Options::default()
        },
    )
    .await;

    // Watch pg_stat_activity from outside, for the server's own role.
    let watcher = admin().await;
    let stop = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicUsize::new(0));
    let (flag, high) = (stop.clone(), peak.clone());
    let sampling = tokio::spawn(async move {
        while !flag.load(Ordering::Relaxed) {
            let n: i64 = watcher
                .query_one(
                    "select count(*) from pg_stat_activity where usename = $1",
                    &[&LOAD.0],
                )
                .await
                .unwrap()
                .get(0);
            high.fetch_max(usize::try_from(n).unwrap(), Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    for i in 0..1000 {
        let reply = get(
            server.addr,
            &query_url("by_region", "region=Andes&from=2024-01-20"),
        )
        .await;
        assert_eq!(reply.status, 200, "request {i}: {}", reply.body);
    }
    let addr = server.addr;
    let wave: Vec<_> = (0..200)
        .map(|_| {
            tokio::spawn(async move { get(addr, &query_url("slow", "seconds=0.02")).await.status })
        })
        .collect();
    for request in wave {
        assert_eq!(request.await.unwrap(), 200);
    }

    stop.store(true, Ordering::Relaxed);
    sampling.await.unwrap();
    let peak = peak.load(Ordering::Relaxed);
    eprintln!("peak connections for the server's role: {peak} (pool size {POOL})");
    assert!(
        peak >= 2,
        "the load never used more than one connection ({peak}): the test proves nothing"
    );
    assert!(
        peak <= POOL,
        "{peak} connections were open, the pool allows {POOL}"
    );
}

/// How many sessions of `role` are in each state, for the states named.
async fn sessions(watcher: &Client, role: &str, state_like: &str) -> i64 {
    watcher
        .query_one(
            "select count(*) from pg_stat_activity where usename = $1 and state like $2",
            &[&role, &state_like],
        )
        .await
        .unwrap()
        .get(0)
}

#[tokio::test]
async fn a_client_that_disconnects_mid_query_does_not_wedge_the_pool() {
    // One connection, so whatever happens to it decides the next request.
    let server = start(
        API,
        Options {
            role: DISCONNECT,
            pool_size: 1,
            acquire_ms: 10_000,
            ..Options::default()
        },
    )
    .await;
    let watcher = admin().await;

    // Ask for a 2 second statement, then hang up without reading the answer.
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let request = format!(
        "GET {} HTTP/1.1\r\nhost: test\r\nconnection: close\r\n\r\n",
        query_url("slow", "seconds=2")
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    drop(stream);
    tokio::time::sleep(Duration::from_millis(400)).await;

    // The statement is not cancelled: it still runs, and holds the only slot.
    // (This is a known gap, written in the README; the test pins it so that a
    // change to it is a decision.)
    assert_eq!(sessions(&watcher, DISCONNECT.0, "active").await, 1);

    // The next request waits for the slot, then is answered normally.
    let started = Instant::now();
    let reply = get(server.addr, &query_url("by_region", "region=Andes")).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(
        started.elapsed() < Duration::from_secs(9),
        "the slot was not given back in time"
    );

    // The abandoned transaction was rolled back, not left open.
    assert_eq!(
        sessions(&watcher, DISCONNECT.0, "idle in transaction%").await,
        0
    );
    assert_eq!(sessions(&watcher, DISCONNECT.0, "active").await, 0);
    assert_eq!(
        get(server.addr, &query_url("by_region", "region=Andes"))
            .await
            .status,
        200
    );
}

// ------------------------------------------------------------------ pages

const BOARD: &str = "examples/earthquakes";

/// The policy of the design, word for word: it is never weakened to make a
/// library work.
const POLICY: &str = "content-security-policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn assert_page_headers(reply: &Reply) {
    assert_eq!(reply.status, 200, "{}", reply.head);
    for header in [
        "content-type: text/html; charset=utf-8",
        POLICY,
        "x-content-type-options: nosniff",
        "referrer-policy: no-referrer",
    ] {
        assert!(
            reply.head.contains(header),
            "missing `{header}` in\n{}",
            reply.head
        );
    }
}

/// The `src` of every script and the `href` of every stylesheet of a page.
fn page_files(html: &str) -> Vec<String> {
    let mut files = Vec::new();
    for marker in ["src=\"", "href=\"/assets/"] {
        for part in html.split(marker).skip(1) {
            let value = part.split('"').next().unwrap();
            let value = if marker.starts_with("href") {
                format!("/assets/{value}")
            } else {
                value.to_string()
            };
            files.push(value);
        }
    }
    files
}

#[tokio::test]
async fn the_index_lists_the_boards_under_the_policy() {
    let server = start(BOARD, Options::default()).await;
    let reply = get(server.addr, "/").await;
    assert_page_headers(&reply);
    assert!(
        reply
            .body
            .contains("<a href=\"/boards/earthquakes\">Earthquakes, January 2024</a>"),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn a_board_page_has_the_policy_and_no_inline_code() {
    let server = start(BOARD, Options::default()).await;
    let page = get(server.addr, "/boards/earthquakes").await;
    assert_page_headers(&page);
    // The policy forbids inline script and style, so the page has none.
    assert!(!page.body.contains("<script>"), "{}", page.body);
    assert!(!page.body.contains("<style"), "{}", page.body);
    assert!(!page.body.contains(" style="), "{}", page.body);
    assert!(!page.body.contains(" onclick="), "{}", page.body);
    // Nothing is loaded from another host.
    assert!(!page.body.contains("http://") && !page.body.contains("https://"));
    let files = page_files(&page.body).join(" ");
    assert!(files.contains("/echarts.min.js") && files.contains("/app.js"));
}

#[tokio::test]
async fn assets_are_served_under_their_hash_and_cached_for_a_year() {
    let server = start(BOARD, Options::default()).await;
    let page = get(server.addr, "/boards/earthquakes").await;
    let files = page_files(&page.body);
    let app = files.iter().find(|f| f.ends_with("/app.js")).unwrap();
    let css = files.iter().find(|f| f.ends_with("/page.css")).unwrap();
    let hash = app.split('/').nth(2).unwrap().to_string();
    assert_eq!(hash.len(), 16);

    let reply = get(server.addr, app).await;
    assert_eq!(reply.status, 200);
    assert!(
        reply
            .head
            .contains("content-type: text/javascript; charset=utf-8")
    );
    assert!(
        reply
            .head
            .contains("cache-control: public, max-age=31536000, immutable")
    );
    assert!(reply.head.contains("x-content-type-options: nosniff"));
    assert_eq!(reply.body, include_str!("../web/app.js"));
    let reply = get(server.addr, css).await;
    assert!(
        reply.head.contains("content-type: text/css; charset=utf-8"),
        "{}",
        reply.head
    );

    // A vendored library is served byte for byte.
    let library = get(server.addr, &format!("/assets/{hash}/echarts.min.js")).await;
    assert_eq!(library.status, 200);
    assert_eq!(
        library.body.len() as u64,
        std::fs::metadata("vendor/echarts.min.js").unwrap().len()
    );

    // Another hash, another name, and a path out of the table are 404s.
    for target in [
        "/assets/0000000000000000/app.js".to_string(),
        format!("/assets/{hash}/missing.js"),
        format!("/assets/{hash}/..%2FCargo.toml"),
        format!("/assets/{hash}/%2E%2E%2Fsrc%2Fmain.rs"),
    ] {
        let reply = get(server.addr, &target).await;
        assert_eq!(reply.status, 404, "{target}");
        assert_eq!(reply.error_code(), "not_found");
    }
}

#[tokio::test]
async fn every_file_a_script_imports_is_embedded() {
    let server = start(BOARD, Options::default()).await;
    let page = get(server.addr, "/boards/earthquakes").await;
    let files = page_files(&page.body);
    let app = files.iter().find(|f| f.ends_with("/app.js")).unwrap();
    let base = app.rsplit_once('/').unwrap().0.to_string();

    // Follow the relative paths ("./name") from app.js through every module.
    let mut todo = vec!["app.js".to_string()];
    let mut seen = Vec::new();
    while let Some(name) = todo.pop() {
        if seen.contains(&name) {
            continue;
        }
        let reply = get(server.addr, &format!("{base}/{name}")).await;
        assert_eq!(reply.status, 200, "{name} is imported but not served");
        if name.ends_with(".js") || name.ends_with(".mjs") {
            for part in reply.body.split("\"./").skip(1) {
                todo.push(part.split('"').next().unwrap().to_string());
            }
        }
        seen.push(name);
    }
    assert!(seen.len() >= 8, "{seen:?}");
    assert!(seen.contains(&"world-110m.json".to_string()));
}

#[tokio::test]
async fn the_definition_tells_the_page_what_the_board_is() {
    let server = start(BOARD, Options::default()).await;
    let reply = get(server.addr, "/api/boards/earthquakes").await;
    assert_eq!(reply.status, 200);
    assert!(reply.head.contains("content-type: application/json"));
    assert!(reply.head.contains("cache-control: no-store"));
    let board = reply.json();
    assert_eq!(board["id"], "earthquakes");
    assert_eq!(board["title"], "Earthquakes, January 2024");

    let filters = board["filters"].as_array().unwrap();
    assert_eq!(filters.len(), 3);
    assert_eq!(filters[0]["control"], "date");
    assert_eq!(filters[0]["type"], "date");
    assert_eq!(filters[0]["default"], "2024-01-01");
    assert_eq!(filters[1]["control"], "multiselect");
    assert_eq!(filters[1]["type"], "list<text>");
    assert_eq!(filters[1]["options_from"]["query"], "regions");
    assert_eq!(filters[1]["default"].as_array().unwrap().len(), 6);
    assert_eq!(
        filters[2]["options"][1],
        json!({"value": 4, "label": "4 and above"})
    );

    let stats = board["stats"].as_array().unwrap();
    assert_eq!(stats.len(), 4);
    assert_eq!(stats[0]["hero"], true);
    assert_eq!(stats[1]["format"], "one_decimal");

    let charts = board["charts"].as_array().unwrap();
    assert_eq!(charts.len(), 3);
    assert_eq!(charts[0]["palette"], "ordered");
    assert_eq!(charts[1]["palette"], "categorical");
    // The specification goes out as written: no data in it.
    assert!(!board["charts"].to_string().contains("\"dataset\""));

    assert_eq!(
        board["queries"]["daily"]["params"]["min_magnitude"]["type"],
        "number"
    );
    assert_eq!(board["queries"]["regions"]["params"], json!({}));
    // No SQL, and no table name, reaches the browser.
    let lower = reply.body.to_lowercase();
    assert!(
        !lower.contains("earthquake_events") && !lower.contains(" where "),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn an_unknown_board_is_a_404_for_the_page_and_the_definition() {
    let server = start(BOARD, Options::default()).await;
    for target in ["/boards/nope", "/api/boards/nope"] {
        let reply = get(server.addr, target).await;
        assert_eq!(reply.status, 404, "{target}");
        assert_eq!(reply.error_code(), "not_found");
    }
}

#[tokio::test]
async fn the_example_queries_answer_over_the_invented_events() {
    let server = start(BOARD, Options::default()).await;
    let base = "/api/boards/earthquakes/queries";

    let regions = get(server.addr, &format!("{base}/regions")).await.json();
    assert_eq!(regions["rows"].as_array().unwrap().len(), 6);
    assert_eq!(
        regions["rows"][0],
        json!({"value": "Aegean", "label": "Aegean"})
    );

    // Every one of the 360 events is in exactly one magnitude class of one day.
    let daily = get(server.addr, &format!("{base}/daily")).await.json();
    assert_eq!(
        daily["columns"],
        json!(["day", "Under 4", "4 to 5", "5 to 6", "6 and over"])
    );
    let rows = daily["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    let total: i64 = rows
        .iter()
        .flat_map(|r| ["Under 4", "4 to 5", "5 to 6", "6 and over"].map(|c| r[c].as_i64().unwrap()))
        .sum();
    assert_eq!(total, 360);

    // The filters change the numbers: one region, and strong events only.
    let one = get(server.addr, &format!("{base}/summary?regions=Andes"))
        .await
        .json();
    let andes = one["rows"][0]["events"].as_i64().unwrap();
    assert!(andes > 0 && andes < 360, "{one}");
    assert_eq!(one["rows"][0]["regions"], 1);
    let strong = get(server.addr, &format!("{base}/summary?min_magnitude=6"))
        .await
        .json();
    assert!(strong["rows"][0]["strongest"].as_f64().unwrap() >= 6.0);
    // A start date past the last event leaves no day: an empty result, which the
    // page says in words.
    let none = get(server.addr, &format!("{base}/daily?from=2030-01-01"))
        .await
        .json();
    assert_eq!(none["rows"], json!([]));
}
