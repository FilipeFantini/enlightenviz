//! The connection to Postgres: a bounded pool, and the two things done with
//! it. `check` prepares every query of the boards before they are served, and
//! `run` answers one request.
//!
//! Every statement runs in a transaction that is `READ ONLY` and has a
//! statement timeout. The author's SQL is wrapped as a subquery so that
//! Postgres itself writes each row as JSON (`row_to_json`): its type mapping
//! (numbers, dates, arrays, nested JSON) is not rewritten here. A statement
//! that writes cannot be wrapped (a data-modifying `WITH` must be at the top
//! level), so it fails at `check`, and the read-only transaction is a second
//! barrier for a function that writes.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use deadpool_postgres::{
    Manager, ManagerConfig, Pool, PoolError, RecyclingMethod, Runtime, TimeoutType,
};
use tokio_postgres::NoTls;
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::types::ToSql;

use crate::catalog::Catalog;
use crate::diagnostic::Diagnostic;
use crate::source::child;
use crate::vega;

/// How to reach Postgres, and the limits every statement runs under.
pub struct Settings {
    /// The connection string. Never printed.
    pub url: String,
    pub pool_size: usize,
    pub acquire_timeout: Duration,
    pub statement_timeout: Duration,
    pub row_limit: usize,
}

pub struct Database {
    pool: Pool,
    statement_timeout_ms: String,
    row_limit: usize,
}

/// A query's answer: the column names, and each row as a JSON object written
/// by Postgres.
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<String>,
}

/// Why a query did not answer. The detail is for the server's log only.
#[derive(Debug)]
pub enum RunError {
    /// Every connection of the pool stayed busy for the whole acquire timeout.
    Busy,
    /// No connection could be made.
    Unavailable(String),
    /// The statement ran past the statement timeout.
    Timeout,
    /// The result has more rows than the limit.
    RowLimit,
    /// Anything else Postgres or the connection reported.
    Failed(String),
}

/// The SQL that runs for a query: the author's SQL as a subquery, one JSON
/// text per row, and one row more than the limit so that an excess is seen.
/// The closing parenthesis is on its own line so that a trailing `--` comment
/// cannot swallow it, and the opening one shares a line with the SQL so that
/// line numbers in Postgres' messages stay the author's.
pub fn wrap(sql: &str, row_limit: usize) -> String {
    format!(
        "select row_to_json(q.*)::text from ({sql}\n) q limit {}",
        row_limit.saturating_add(1)
    )
}

impl Database {
    /// Builds the pool. No connection is opened until the first use.
    pub fn new(settings: &Settings) -> Result<Database, String> {
        let mut config = tokio_postgres::Config::from_str(&settings.url)
            .map_err(|e| format!("the database URL is not valid: {e}"))?;
        if config.get_application_name().is_none() {
            config.application_name("enlightenviz");
        }
        let manager = Manager::from_config(
            config,
            NoTls,
            ManagerConfig {
                recycling_method: RecyclingMethod::Verified,
            },
        );
        let pool = Pool::builder(manager)
            .max_size(settings.pool_size)
            .wait_timeout(Some(settings.acquire_timeout))
            .create_timeout(Some(settings.acquire_timeout))
            .recycle_timeout(Some(settings.acquire_timeout))
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|e| format!("the connection pool cannot be built: {e}"))?;
        Ok(Database {
            pool,
            statement_timeout_ms: settings.statement_timeout.as_millis().to_string(),
            row_limit: settings.row_limit,
        })
    }

    /// Runs a compiled query with its parameters bound as text.
    pub async fn run(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Rows, RunError> {
        let mut client = self.pool.get().await.map_err(pool_error)?;
        let tx = client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(failed)?;
        tx.query(
            "select set_config('statement_timeout', $1, true)",
            &[&self.statement_timeout_ms],
        )
        .await
        .map_err(failed)?;
        // The columns are read from this transaction's catalog on every
        // request, so a schema change cannot leave them out of step with rows.
        let described = tx.prepare(sql).await.map_err(failed)?;
        let wrapped = tx
            .prepare_cached(&wrap(sql, self.row_limit))
            .await
            .map_err(failed)?;
        let found = tx.query(&wrapped, params).await.map_err(failed)?;
        tx.commit().await.map_err(failed)?;
        if found.len() > self.row_limit {
            return Err(RunError::RowLimit);
        }
        let rows = found
            .iter()
            .map(|row| row.try_get::<_, String>(0).map_err(failed))
            .collect::<Result<Vec<_>, _>>()?;
        let columns = described
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();
        Ok(Rows { columns, rows })
    }

    /// Prepares every compiled query inside a read-only transaction and
    /// checks what the database can tell: that the SQL is valid, that its
    /// columns have distinct names, and that every field a chart draws is one
    /// of them. A connection that cannot be made is one diagnostic.
    pub async fn check(&self, catalog: &Catalog) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut client = match self.pool.get().await {
            Ok(client) => client,
            Err(e) => {
                diagnostics.push(unreachable_diagnostic(&pool_error_detail(&e)));
                return diagnostics;
            }
        };
        for loaded in &catalog.boards {
            for (name, compiled) in &loaded.queries {
                let sql_line = loaded.doc.line(&child(&child("/queries", name), "sql"));
                let tx = match client.build_transaction().read_only(true).start().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        diagnostics.push(unreachable_diagnostic(&failed_detail(&e)));
                        return diagnostics;
                    }
                };
                if let Err(e) = tx
                    .query(
                        "select set_config('statement_timeout', $1, true)",
                        &[&self.statement_timeout_ms],
                    )
                    .await
                {
                    diagnostics.push(unreachable_diagnostic(&failed_detail(&e)));
                    return diagnostics;
                }
                let described = match tx.prepare(&compiled.sql).await {
                    Ok(described) => described,
                    Err(e) => {
                        diagnostics.push(prepare_diagnostic(
                            &loaded.file,
                            sql_line,
                            &compiled.sql,
                            &e,
                        ));
                        continue; // the transaction is dropped, which rolls it back
                    }
                };
                if let Err(e) = tx.prepare(&wrap(&compiled.sql, self.row_limit)).await {
                    diagnostics.push(not_a_select_diagnostic(&loaded.file, sql_line, &e));
                    continue;
                }
                if let Err(e) = tx.rollback().await {
                    diagnostics.push(unreachable_diagnostic(&failed_detail(&e)));
                    return diagnostics;
                }

                let columns: Vec<&str> = described.columns().iter().map(|c| c.name()).collect();
                let mut seen = HashSet::new();
                for column in &columns {
                    if !seen.insert(*column) {
                        diagnostics.push(Diagnostic::new(
                            &loaded.file,
                            sql_line,
                            "query-column-duplicate",
                            format!("the query `{name}` returns more than one column named `{column}`; a row is a JSON object, so one would be lost"),
                            "give each column its own name with `as`",
                        ));
                    }
                }
                let columns: HashSet<&str> = columns.into_iter().collect();
                for (index, chart) in loaded.board.charts.iter().enumerate() {
                    if chart.query != *name {
                        continue;
                    }
                    let at = format!("/charts/{index}/vega_lite");
                    let fields =
                        vega::collect(&serde_json::Value::Object(chart.vega_lite.clone()), &at);
                    for (pointer, field) in &fields.used {
                        if !vega::is_known(field, &columns, &fields.created) {
                            let mut have: Vec<&str> = columns.iter().copied().collect();
                            have.sort_unstable();
                            diagnostics.push(Diagnostic::new(
                                &loaded.file,
                                loaded.doc.line(pointer),
                                "chart-unknown-field",
                                format!("the chart `{}` draws the field `{field}`, which the query `{name}` does not return and no transform creates with `as`", chart.title),
                                format!("use one of the columns ({}), or add `as: {field}` to the transform that creates it", have.join(", ")),
                            ));
                        }
                    }
                }
            }
        }
        diagnostics
    }
}

fn pool_error(e: PoolError) -> RunError {
    match e {
        PoolError::Timeout(TimeoutType::Wait) => RunError::Busy,
        other => RunError::Unavailable(pool_error_detail(&other)),
    }
}

fn pool_error_detail(e: &PoolError) -> String {
    match e {
        PoolError::Backend(inner) => failed_detail(inner),
        other => other.to_string(),
    }
}

fn failed(e: tokio_postgres::Error) -> RunError {
    if e.as_db_error()
        .is_some_and(|d| *d.code() == SqlState::QUERY_CANCELED)
    {
        RunError::Timeout
    } else {
        RunError::Failed(failed_detail(&e))
    }
}

/// What Postgres said, with its SQLSTATE; for logs. A connection error is
/// reported by its own message, which never holds the connection string.
fn failed_detail(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => format!("{} (SQLSTATE {})", db.message(), db.code().code()),
        None => e.to_string(),
    }
}

fn unreachable_diagnostic(detail: &str) -> Diagnostic {
    Diagnostic::new(
        "database",
        None,
        "database-unreachable",
        format!("the database cannot be used: {detail}"),
        "check the database URL, that the server is up, and that the role may log in",
    )
}

/// The statement prepares on its own but not as a subquery: it is not a
/// `SELECT`, or it holds a data-modifying `WITH`.
fn not_a_select_diagnostic(
    file: &str,
    line: Option<usize>,
    e: &tokio_postgres::Error,
) -> Diagnostic {
    Diagnostic::new(
        file,
        line,
        "sql-not-a-select",
        format!(
            "the statement cannot be run as a read-only query: {}",
            failed_detail(e)
        ),
        "write a SELECT, or a WITH ... SELECT whose WITH parts only read; no INSERT, UPDATE or DELETE",
    )
}

fn prepare_diagnostic(
    file: &str,
    line: Option<usize>,
    sql: &str,
    e: &tokio_postgres::Error,
) -> Diagnostic {
    let Some(db) = e.as_db_error() else {
        return unreachable_diagnostic(&e.to_string());
    };
    if *db.code() == SqlState::QUERY_CANCELED {
        return Diagnostic::new(
            file,
            line,
            "sql-prepare-timeout",
            "Postgres took longer than the statement timeout to prepare the query",
            "check the database, or raise the statement timeout",
        );
    }
    let place = match db.position() {
        Some(ErrorPosition::Original(at)) => {
            let line_in_query = sql
                .chars()
                .take(at.saturating_sub(1) as usize)
                .filter(|c| *c == '\n')
                .count()
                + 1;
            format!(" (line {line_in_query} of the query)")
        }
        _ => String::new(),
    };
    Diagnostic::new(
        file,
        line,
        "sql-prepare-failed",
        format!(
            "Postgres cannot prepare the query{place}: {} (SQLSTATE {})",
            db.message(),
            db.code().code()
        ),
        "fix the SQL; a query must be one SELECT, or a WITH ... SELECT, over tables the role may read",
    )
}
