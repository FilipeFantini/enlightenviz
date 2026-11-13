//! Loads a directory of boards and checks everything that can be checked
//! without a database: the YAML, the shape of the board, the parameters, the
//! SQL, and which query each chart reads.
//!
//! Every problem found is a [`Diagnostic`], and every board is checked even
//! when another one failed, so that one run reports as much as it can.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use serde_path_to_error::Segment;

use crate::board::{Board, valid_name};
use crate::diagnostic::Diagnostic;
use crate::param::{ParamSpec, ParamType};
use crate::source::{Document, child, parse};
use crate::sql::{self, Problem};

/// A board file larger than this is refused.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The boards that loaded, with their queries ready to run.
#[derive(Debug)]
pub struct Catalog {
    pub boards: Vec<LoadedBoard>,
}

#[derive(Debug)]
pub struct LoadedBoard {
    pub file: String,
    pub doc: Document,
    pub board: Board,
    /// The queries that compiled; a query with an error is absent.
    pub queries: BTreeMap<String, CompiledQuery>,
}

/// A query whose parameters are declared and whose SQL is rewritten.
#[derive(Debug)]
pub struct CompiledQuery {
    pub params: BTreeMap<String, ParamSpec>,
    /// Parameter names in positional order: `order[0]` is `$1`.
    pub order: Vec<String>,
    /// The SQL with positional parameters, without a trailing `;`.
    pub sql: String,
}

/// Loads every `.yml` and `.yaml` file directly inside `dir`, in name order.
pub fn load_dir(dir: &Path) -> (Catalog, Vec<Diagnostic>) {
    let shown = dir.display().to_string();
    let mut diagnostics = Vec::new();
    let mut boards = Vec::new();

    let mut files = match list_boards(dir) {
        Ok(files) => files,
        Err(message) => {
            let fix = "pass a directory that exists and can be read";
            diagnostics.push(Diagnostic::new(
                &shown,
                None,
                "boards-dir-unreadable",
                message,
                fix,
            ));
            return (Catalog { boards }, diagnostics);
        }
    };
    files.sort();
    if files.is_empty() {
        diagnostics.push(Diagnostic::new(
            &shown,
            None,
            "boards-dir-empty",
            "the directory holds no board: no file ends in .yml or .yaml",
            "add a board file, or point to the directory that holds them",
        ));
    }

    let mut ids: HashMap<String, String> = HashMap::new();
    for path in files {
        let file = path.display().to_string();
        let Some(loaded) = load_file(&path, &file, &mut diagnostics) else {
            continue;
        };
        match ids.get(&loaded.board.board) {
            Some(first) => diagnostics.push(Diagnostic::new(
                &file,
                loaded.doc.line("/board"),
                "board-duplicate",
                format!(
                    "the board id `{}` is already used by {first}",
                    loaded.board.board
                ),
                "give each board its own id",
            )),
            None => {
                ids.insert(loaded.board.board.clone(), file);
            }
        }
        boards.push(loaded);
    }
    (Catalog { boards }, diagnostics)
}

fn list_boards(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("the directory cannot be read: {e}"))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("the directory cannot be read: {e}"))?
            .path();
        let is_board =
            path.extension().is_some_and(|e| e == "yml" || e == "yaml") && path.is_file();
        if is_board {
            files.push(path);
        }
    }
    Ok(files)
}

fn load_file(path: &Path, file: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<LoadedBoard> {
    let mut text = String::new();
    let read =
        fs::File::open(path).and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_string(&mut text));
    if let Err(e) = read {
        diagnostics.push(Diagnostic::new(
            file,
            None,
            "file-unreadable",
            format!("the file cannot be read as UTF-8 text: {e}"),
            "check that the file exists, can be read, and is UTF-8",
        ));
        return None;
    }
    if text.len() as u64 > MAX_FILE_BYTES {
        diagnostics.push(Diagnostic::new(
            file,
            None,
            "file-too-large",
            format!("the file is larger than {MAX_FILE_BYTES} bytes"),
            "split the board into smaller boards",
        ));
        return None;
    }

    let mut doc = match parse(&text) {
        Ok(doc) => doc,
        Err(e) => {
            diagnostics.push(Diagnostic::new(file, e.line, e.rule, e.message, e.fix));
            return None;
        }
    };
    let value = std::mem::take(&mut doc.value);
    let board: Board = match serde_path_to_error::deserialize(value) {
        Ok(board) => board,
        Err(e) => {
            diagnostics.push(schema_diagnostic(file, &doc, &e));
            return None;
        }
    };
    let queries = compile_board(file, &doc, &board, diagnostics);
    Some(LoadedBoard {
        file: file.to_string(),
        doc,
        board,
        queries,
    })
}

/// A value that does not fit the board format, located by the path serde
/// followed to it.
fn schema_diagnostic(
    file: &str,
    doc: &Document,
    error: &serde_path_to_error::Error<serde_json::Error>,
) -> Diagnostic {
    let mut pointer = String::new();
    for segment in error.path().iter() {
        match segment {
            Segment::Seq { index } => pointer = child(&pointer, &index.to_string()),
            Segment::Map { key } => pointer = child(&pointer, key),
            Segment::Enum { variant } => pointer = child(&pointer, variant),
            Segment::Unknown => break,
        }
    }
    let message = error.inner().to_string();
    // "unknown field `x`, ..." names the field, which has a line of its own.
    let named = message
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split_once('`'))
        .map(|(name, _)| child(&pointer, name));
    let line = named
        .and_then(|p| doc.line(&p))
        .or_else(|| doc.line(&pointer));
    let fix = if message.starts_with("unknown field") {
        "remove the field, or fix its name (see `enlightenviz schema`)"
    } else if message.starts_with("missing field") {
        "add the missing field"
    } else {
        "change the value to what the schema expects (see `enlightenviz schema`)"
    };
    let place = if pointer.is_empty() {
        "the board".to_string()
    } else {
        error.path().to_string()
    };
    Diagnostic::new(file, line, "schema", format!("{place}: {message}"), fix)
}

fn compile_board(
    file: &str,
    doc: &Document,
    board: &Board,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, CompiledQuery> {
    if !valid_name(&board.board, true) {
        diagnostics.push(name_diagnostic(
            file,
            doc.line("/board"),
            "board id",
            &board.board,
            true,
        ));
    }
    let mut compiled = BTreeMap::new();
    for (name, query) in &board.queries {
        let at = child("/queries", name);
        if !valid_name(name, true) {
            diagnostics.push(name_diagnostic(
                file,
                doc.line(&at),
                "query name",
                name,
                true,
            ));
        }
        if let Some(c) = compile_query(file, doc, &at, query, diagnostics) {
            compiled.insert(name.clone(), c);
        }
    }
    if board.charts.is_empty() {
        diagnostics.push(Diagnostic::new(
            file,
            doc.line("/charts"),
            "board-no-charts",
            "the board has no chart",
            "add at least one chart under `charts`",
        ));
    }
    for (index, chart) in board.charts.iter().enumerate() {
        if !board.queries.contains_key(&chart.query) {
            let known: Vec<&str> = board.queries.keys().map(String::as_str).collect();
            diagnostics.push(Diagnostic::new(
                file,
                doc.line(&child(&child("/charts", &index.to_string()), "query")),
                "chart-unknown-query",
                format!(
                    "the chart `{}` reads the query `{}`, which the board does not declare",
                    chart.title, chart.query
                ),
                format!("use one of the declared queries: {}", known.join(", ")),
            ));
        }
    }
    compiled
}

fn name_diagnostic(
    file: &str,
    line: Option<usize>,
    what: &str,
    name: &str,
    dash: bool,
) -> Diagnostic {
    let allowed = if dash {
        "letters, digits, `_` and `-`"
    } else {
        "letters, digits and `_`"
    };
    Diagnostic::new(
        file,
        line,
        "name-invalid",
        format!(
            "the {what} `{name}` is not valid: use {allowed}, starting with a letter or `_`, at most 63 characters"
        ),
        "rename it",
    )
}

fn compile_query(
    file: &str,
    doc: &Document,
    at: &str,
    query: &crate::board::Query,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledQuery> {
    let errors_before = diagnostics.len();
    let params_at = child(at, "params");
    let mut types: BTreeMap<String, ParamType> = BTreeMap::new();
    let mut specs: BTreeMap<String, ParamSpec> = BTreeMap::new();

    for (name, param) in &query.params {
        let param_at = child(&params_at, name);
        if !valid_name(name, false) {
            diagnostics.push(name_diagnostic(
                file,
                doc.line(&param_at),
                "parameter name",
                name,
                false,
            ));
        }
        types.insert(name.clone(), param.ty);
        match ParamSpec::declare(param.ty, param.default.as_ref(), param.max_items) {
            Ok(spec) => {
                specs.insert(name.clone(), spec);
            }
            Err(e) => {
                let field = if e.rule == "param-default-invalid" {
                    "default"
                } else {
                    "max_items"
                };
                let line = doc.line(&child(&param_at, field));
                diagnostics.push(Diagnostic::new(
                    file,
                    line,
                    e.rule,
                    format!("parameter `{name}`: {}", e.message),
                    e.fix,
                ));
            }
        }
    }

    let sql_line = doc.line(&child(at, "sql"));
    let rewritten = match sql::rewrite(&query.sql, &types) {
        Ok(rewritten) => rewritten,
        Err(failure) => {
            for e in failure.errors {
                diagnostics.push(sql_diagnostic(file, sql_line, e));
            }
            if failure.omitted > 0 {
                diagnostics.push(Diagnostic::new(
                    file,
                    sql_line,
                    "sql-errors-truncated",
                    format!(
                        "{} more problems in this query are not shown; the first {} are listed",
                        failure.omitted,
                        sql::MAX_ERRORS
                    ),
                    "fix the problems shown and run again; the rest will then be listed",
                ));
            }
            return None;
        }
    };
    let used: HashSet<&String> = rewritten.order.iter().collect();
    for name in types.keys().filter(|n| !used.contains(n)) {
        diagnostics.push(Diagnostic::new(
            file,
            doc.line(&child(&params_at, name)),
            "sql-unused-param",
            format!("the parameter `{name}` is declared but the SQL never uses it"),
            format!("use `${name}` in the SQL, or remove the declaration"),
        ));
    }
    if diagnostics.len() > errors_before {
        return None;
    }
    Some(CompiledQuery {
        params: specs,
        order: rewritten.order,
        sql: rewritten.sql,
    })
}

fn sql_diagnostic(file: &str, line: Option<usize>, error: sql::SqlError) -> Diagnostic {
    let n = error.line;
    let (rule, message, fix) = match error.problem {
        Problem::Undeclared(name) => (
            "sql-undeclared-param",
            format!(
                "the SQL uses `${name}` (line {n} of the query), which is not declared under `params`"
            ),
            format!(
                "declare `{name}` under `params`, or remove it; a literal `$` belongs inside a string"
            ),
        ),
        Problem::Positional(position) => (
            "sql-positional-param",
            format!("the SQL uses the positional parameter `${position}` (line {n} of the query)"),
            "write a named parameter, `$name`, and declare it under `params`".to_string(),
        ),
        Problem::Unterminated(what) => (
            "sql-unterminated",
            format!("the SQL has a {what} that never closes, starting at line {n} of the query"),
            format!("close the {what}"),
        ),
        Problem::ExtraStatement => (
            "sql-multiple-statements",
            format!(
                "the SQL has a `;` (line {n} of the query) followed by more SQL; a query is one statement"
            ),
            "keep one statement per query, and split the rest into other queries".to_string(),
        ),
        Problem::Empty => (
            "sql-empty",
            "the SQL has no statement, only whitespace and comments".to_string(),
            "write the query".to_string(),
        ),
    };
    Diagnostic::new(file, line, rule, message, fix)
}
