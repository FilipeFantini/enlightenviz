//! The HTTP side: health, and a query's result as JSON.
//!
//! The browser names a board and a query and gives typed values; it never
//! sends SQL. Each value is checked against its declared type before it
//! reaches the database. Nothing from the database, and no SQL, is ever sent
//! to the client: a failure is logged here with an id, and the client gets the
//! same id.

use std::collections::BTreeMap;
use std::path::Path as FsPath;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio_postgres::types::ToSql;

use crate::catalog::{CompiledQuery, load_dir};
use crate::db::{Database, RunError, Settings};
use crate::diagnostic::Diagnostic;
use crate::log;
use crate::param::Bound;

/// What the handlers share.
pub struct AppState {
    db: Database,
    row_limit: usize,
    /// board id -> query name -> the query.
    queries: BTreeMap<String, BTreeMap<String, CompiledQuery>>,
}

impl AppState {
    /// Loads and checks the boards in `boards`, prepares every query against
    /// the database, and returns the state to serve. Any problem stops here.
    pub async fn prepare(boards: &FsPath, settings: &Settings) -> Result<AppState, PrepareError> {
        let (catalog, mut diagnostics) = load_dir(boards);
        let db = Database::new(settings).map_err(PrepareError::Failed)?;
        diagnostics.extend(db.check(&catalog).await);
        if !diagnostics.is_empty() {
            diagnostics.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
            return Err(PrepareError::Invalid(diagnostics));
        }
        let queries = catalog
            .boards
            .into_iter()
            .map(|b| (b.board.board, b.queries))
            .collect();
        Ok(AppState {
            db,
            row_limit: settings.row_limit,
            queries,
        })
    }

    /// How many boards and queries are served.
    pub fn counts(&self) -> (usize, usize) {
        (
            self.queries.len(),
            self.queries.values().map(BTreeMap::len).sum(),
        )
    }
}

/// Why the server cannot start.
#[derive(Debug)]
pub enum PrepareError {
    /// A board has a problem; each one is a diagnostic.
    Invalid(Vec<Diagnostic>),
    /// The run itself failed.
    Failed(String),
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/boards/{board}/queries/{query}", get(query))
        .fallback(not_found)
        .with_state(state)
}

/// Serves until SIGTERM or SIGINT, then finishes the requests in flight. A
/// process that is PID 1 in a container ignores SIGTERM unless it handles it.
pub async fn serve(listener: TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let stop = async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = interrupt.recv() => {}
        }
        log::info("stopping");
    };
    axum::serve(listener, router(state))
        .with_graceful_shutdown(stop)
        .await
}

async fn healthz() -> &'static str {
    "ok\n"
}

async fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "there is nothing at this address",
    )
}

async fn query(
    State(state): State<Arc<AppState>>,
    Path((board, name)): Path<(String, String)>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    let queries = state.queries.get(&board).ok_or_else(|| {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", "there is no such board")
    })?;
    let compiled = queries.get(&name).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "the board has no such query",
        )
    })?;

    let bound = bind(compiled, raw.as_deref().unwrap_or(""))?;
    let refs: Vec<&(dyn ToSql + Sync)> = bound
        .iter()
        .map(|b| match b {
            Bound::One(text) => text as &(dyn ToSql + Sync),
            Bound::Many(items) => items as &(dyn ToSql + Sync),
        })
        .collect();

    let context = format!("{board}/{name}");
    let rows = state
        .db
        .run(&compiled.sql, &refs)
        .await
        .map_err(|e| run_error(e, &context, state.row_limit))?;

    let columns = serde_json::to_string(&rows.columns).map_err(|e| {
        ApiError::internal(format!("{context}: the columns cannot be written: {e}"))
    })?;
    let mut body = String::with_capacity(
        32 + columns.len() + rows.rows.iter().map(|r| r.len() + 1).sum::<usize>(),
    );
    body.push_str("{\"columns\":");
    body.push_str(&columns);
    body.push_str(",\"rows\":[");
    for (index, row) in rows.rows.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(row);
    }
    body.push_str("]}");
    Ok(json_response(StatusCode::OK, body))
}

/// Checks the query string against the declared parameters, and returns one
/// value per positional parameter. The values are text: Postgres casts them.
fn bind(compiled: &CompiledQuery, raw: &str) -> Result<Vec<Bound>, ApiError> {
    let mut given: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
        // `parse` replaces bytes that are not UTF-8 with U+FFFD; a value that
        // arrived damaged is refused rather than used.
        if key.contains('\u{FFFD}') || value.contains('\u{FFFD}') {
            return Err(ApiError::bad_request(
                "the query string is not valid UTF-8".to_string(),
            ));
        }
        given
            .entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }
    if let Some(unknown) = given.keys().find(|k| !compiled.params.contains_key(*k)) {
        let shown: String = unknown.chars().take(64).collect();
        return Err(ApiError::bad_request(format!(
            "`{shown}` is not a parameter of this query"
        )));
    }
    compiled
        .order
        .iter()
        .map(|name| {
            let spec = compiled.params.get(name).ok_or_else(|| {
                ApiError::internal(format!("the parameter {name} has no declaration"))
            })?;
            match given.remove(name) {
                Some(values) => spec
                    .bind(&values)
                    .map_err(|why| ApiError::bad_request(format!("parameter `{name}` {why}"))),
                None => spec.default.clone().ok_or_else(|| {
                    ApiError::bad_request(format!("parameter `{name}` is required"))
                }),
            }
        })
        .collect()
}

fn run_error(error: RunError, context: &str, row_limit: usize) -> ApiError {
    match error {
        RunError::Busy => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "the server is busy; try again in a moment",
        )
        .logged(format!(
            "{context}: no database connection was free in time"
        )),
        RunError::Unavailable(detail) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "the database cannot be reached; try again later",
        )
        .logged(format!("{context}: {detail}")),
        RunError::Timeout => ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "query_timeout",
            "the query took longer than the time allowed",
        )
        .logged(format!("{context}: the statement timeout was reached")),
        RunError::RowLimit => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "row_limit",
            format!(
                "the result has more than {row_limit} rows; narrow it with the query's parameters"
            ),
        )
        .logged(format!(
            "{context}: the result is over the row limit of {row_limit}"
        )),
        RunError::Failed(detail) => ApiError::internal(format!("{context}: {detail}")),
    }
}

fn json_response(status: StatusCode, body: String) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// An answer that is an error. `message` goes to the client; `detail` goes to
/// the log, never to the client.
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    detail: Option<String>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorFields<'a>,
}

#[derive(Serialize)]
struct ErrorFields<'a> {
    id: &'a str,
    code: &'a str,
    message: &'a str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code,
            message: message.into(),
            detail: None,
        }
    }

    fn bad_request(message: String) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    fn internal(detail: String) -> ApiError {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "the query failed; give the id to whoever runs the server",
        )
        .logged(detail)
    }

    fn logged(mut self, detail: String) -> ApiError {
        self.detail = Some(detail);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let id = next_id();
        if let Some(detail) = &self.detail {
            log::error(&id, detail);
        }
        let body = ErrorBody {
            error: ErrorFields {
                id: &id,
                code: self.code,
                message: &self.message,
            },
        };
        match serde_json::to_string(&body) {
            Ok(text) => json_response(self.status, text),
            Err(e) => {
                log::error(&id, &format!("an error body cannot be written: {e}"));
                self.status.into_response()
            }
        }
    }
}

/// An id that is unique to one error in one run of the process: when the
/// process started, and a counter.
fn next_id() -> String {
    static STARTED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let started = *STARTED.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    });
    format!("{started:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}
