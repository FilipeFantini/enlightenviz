//! What `validate` checks about a board's charts, filters and stats.
//!
//! [`offline`] needs only the file. [`columns`] needs the columns a query
//! returns, which only a database can say, and checks every place that names
//! one: a chart's `encode`, a stat's value, a filter's option columns.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::board::{Board, Control};
use crate::catalog::LoadedBoard;
use crate::diagnostic::Diagnostic;
use crate::echarts;
use crate::param::{ParamType, Scalar, default_item, type_name};
use crate::source::{Document, child};
use crate::spec;

/// Checks that need no database: the specifications, the filters and the
/// stats.
pub fn offline(file: &str, doc: &Document, board: &Board, diagnostics: &mut Vec<Diagnostic>) {
    for (index, chart) in board.charts.iter().enumerate() {
        let at = child(&child("/charts", &index.to_string()), "echarts");
        let option = Value::Object(chart.echarts.clone());
        let problems = spec::forbidden_in(&option, &at)
            .into_iter()
            .chain(spec::series_problems(&option, &at));
        for problem in problems {
            diagnostics.push(Diagnostic::new(
                file,
                doc.line(&problem.pointer),
                problem.rule,
                format!("the chart `{}` {}", chart.title, problem.what),
                problem.fix,
            ));
        }
    }
    filters(file, doc, board, diagnostics);
    stats(file, doc, board, diagnostics);
}

fn stats(file: &str, doc: &Document, board: &Board, diagnostics: &mut Vec<Diagnostic>) {
    let mut hero: Option<usize> = None;
    for (index, stat) in board.stats.iter().enumerate() {
        let at = child("/stats", &index.to_string());
        if !board.queries.contains_key(&stat.query) {
            diagnostics.push(Diagnostic::new(
                file,
                doc.line(&child(&at, "query")),
                "stat-unknown-query",
                format!(
                    "the stat `{}` reads the query `{}`, which the board does not declare",
                    stat.label, stat.query
                ),
                format!("use one of the declared queries: {}", names(board)),
            ));
        }
        if let Some(trend) = &stat.trend
            && !board.queries.contains_key(&trend.query)
        {
            diagnostics.push(Diagnostic::new(
                file,
                doc.line(&child(&child(&at, "trend"), "query")),
                "stat-unknown-query",
                format!(
                    "the trend of the stat `{}` reads the query `{}`, which the board does not declare",
                    stat.label, trend.query
                ),
                format!("use one of the declared queries: {}", names(board)),
            ));
        }
        if stat.hero {
            match hero {
                Some(first) => diagnostics.push(Diagnostic::new(
                    file,
                    doc.line(&child(&at, "hero")),
                    "stat-hero-multiple",
                    format!(
                        "the stat `{}` is a second hero figure; stat number {} already is one",
                        stat.label,
                        first + 1
                    ),
                    "mark at most one stat with `hero: true`",
                )),
                None => hero = Some(index),
            }
        }
    }
}

fn names(board: &Board) -> String {
    board
        .queries
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn filters(file: &str, doc: &Document, board: &Board, diagnostics: &mut Vec<Diagnostic>) {
    let mut driven: HashMap<&str, usize> = HashMap::new();
    for (index, filter) in board.filters.iter().enumerate() {
        let at = child("/filters", &index.to_string());
        let param_line = doc.line(&child(&at, "param"));
        let label = &filter.label;

        if let Some(first) = driven.insert(&filter.param, index) {
            diagnostics.push(Diagnostic::new(
                file,
                param_line,
                "filter-duplicate",
                format!(
                    "the filters {} and {} both drive the parameter `{}`",
                    first + 1,
                    index + 1,
                    filter.param
                ),
                "keep one filter for the parameter",
            ));
        }

        let declaring: Vec<_> = board
            .queries
            .iter()
            .filter_map(|(name, q)| q.params.get(&filter.param).map(|p| (name, p)))
            .collect();
        let Some(&(first_query, first)) = declaring.first() else {
            diagnostics.push(Diagnostic::new(
                file,
                param_line,
                "filter-unknown-param",
                format!(
                    "the filter `{label}` drives the parameter `{}`, which no query declares",
                    filter.param
                ),
                "name a parameter that a query declares under `params`",
            ));
            continue;
        };

        // A filter sets one value for every query that declares the name, so
        // those declarations must agree on everything the page shows.
        for (name, param) in &declaring[1..] {
            let differs = if param.ty != first.ty {
                Some(format!(
                    "`{}` in `{first_query}` and `{}` in `{name}`",
                    type_name(first.ty),
                    type_name(param.ty)
                ))
            } else if param.default != first.default {
                Some(format!(
                    "different defaults in `{first_query}` and `{name}`"
                ))
            } else if param.max_items != first.max_items {
                Some(format!(
                    "different `max_items` in `{first_query}` and `{name}`"
                ))
            } else {
                None
            };
            if let Some(what) = differs {
                diagnostics.push(Diagnostic::new(
                    file,
                    doc.line(&child(&child(&child("/queries", name), "params"), &filter.param)),
                    "filter-param-conflict",
                    format!(
                        "the filter `{label}` drives `{}` in several queries, which declare it differently: {what}",
                        filter.param
                    ),
                    "declare the parameter the same way (type, default, max_items) in every query that a filter drives",
                ));
            }
        }
        for (name, param) in &declaring {
            if param.default.is_none() {
                diagnostics.push(Diagnostic::new(
                    file,
                    doc.line(&child(&child(&child("/queries", name), "params"), &filter.param)),
                    "filter-param-required",
                    format!(
                        "the parameter `{}` of `{name}` has no default, so the filter `{label}` has no starting value",
                        filter.param
                    ),
                    "add a `default` to the parameter",
                ));
            }
        }

        let fits = match filter.control {
            Control::Select => !first.ty.is_list() && first.ty != ParamType::Date,
            Control::Multiselect => first.ty.is_list(),
            Control::Date => first.ty == ParamType::Date,
        };
        if !fits {
            let (name, needs) = match filter.control {
                Control::Select => ("select", "a text, integer, number or boolean parameter"),
                Control::Multiselect => ("multiselect", "a list parameter"),
                Control::Date => ("date", "a date parameter"),
            };
            diagnostics.push(Diagnostic::new(
                file,
                doc.line(&child(&at, "control")),
                "filter-control-type",
                format!(
                    "the filter `{label}` is a `{name}` control, which needs {needs}, but `{}` is `{}`",
                    filter.param,
                    type_name(first.ty)
                ),
                "change the control, or the type of the parameter: `select` for one value, `multiselect` for a list, `date` for a date",
            ));
        }

        options(
            file,
            doc,
            board,
            index,
            first.ty.scalar(),
            first.default.as_ref(),
            diagnostics,
        );
    }
}

fn options(
    file: &str,
    doc: &Document,
    board: &Board,
    index: usize,
    scalar: Scalar,
    default: Option<&Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let filter = &board.filters[index];
    let at = child("/filters", &index.to_string());
    let label = &filter.label;
    let mut bad = |pointer: String, message: String, fix: &str| {
        diagnostics.push(Diagnostic::new(
            file,
            doc.line(&pointer),
            "filter-options-invalid",
            message,
            fix,
        ));
    };

    if filter.control == Control::Date {
        for key in ["options", "options_from"] {
            let present = if key == "options" {
                filter.options.is_some()
            } else {
                filter.options_from.is_some()
            };
            if present {
                bad(
                    child(&at, key),
                    format!("the filter `{label}` is a date control, which takes no options"),
                    "remove the options",
                );
            }
        }
        return;
    }
    match (&filter.options, &filter.options_from) {
        (None, None) => bad(
            at.clone(),
            format!("the filter `{label}` has no options"),
            "add `options` (a list of value and label) or `options_from` (a query, a value column, a label column)",
        ),
        (Some(_), Some(_)) => bad(
            child(&at, "options_from"),
            format!("the filter `{label}` has both `options` and `options_from`"),
            "keep one of them",
        ),
        (Some(list), None) => {
            if list.is_empty() {
                bad(
                    child(&at, "options"),
                    format!("the filter `{label}` has an empty list of options"),
                    "list at least one option",
                );
            }
            let mut values: Vec<String> = Vec::new();
            for (position, option) in list.iter().enumerate() {
                let value_at = child(
                    &child(&child(&at, "options"), &position.to_string()),
                    "value",
                );
                match default_item(scalar, &option.value) {
                    Ok(text) => {
                        if values.contains(&text) {
                            bad(
                                value_at,
                                format!(
                                    "the option value `{text}` of the filter `{label}` appears twice"
                                ),
                                "list each value once",
                            );
                        }
                        values.push(text);
                    }
                    Err(why) => bad(
                        value_at,
                        format!(
                            "an option of the filter `{label}` is not a valid value of its parameter: {why}"
                        ),
                        "write the value as the parameter's type: a quoted string for text, a number for integer and number, true or false for boolean",
                    ),
                }
            }
            let wanted: Vec<&Value> = match default {
                Some(Value::Array(items)) => items.iter().collect(),
                Some(one) => vec![one],
                None => Vec::new(),
            };
            for value in wanted {
                if let Ok(text) = default_item(scalar, value)
                    && !values.contains(&text)
                {
                    bad(
                        child(&at, "param"),
                        format!(
                            "the default `{text}` of the parameter `{}` is not among the options of the filter `{label}`",
                            filter.param
                        ),
                        "add it to the options, or change the default",
                    );
                }
            }
        }
        (None, Some(from)) => match board.queries.get(&from.query) {
            None => bad(
                child(&child(&at, "options_from"), "query"),
                format!(
                    "the filter `{label}` takes its options from the query `{}`, which the board does not declare",
                    from.query
                ),
                &format!("use one of the declared queries: {}", names(board)),
            ),
            Some(query) => {
                for (name, param) in &query.params {
                    if param.default.is_none() {
                        bad(
                            child(&child(&at, "options_from"), "query"),
                            format!(
                                "the options query `{}` needs the parameter `{name}`, which has no default; options are fetched without parameters",
                                from.query
                            ),
                            "add a default to the parameter, or use a query without parameters",
                        );
                    }
                }
            }
        },
    }
}

/// Checks everything that names a column of the query `query`, which returns
/// `columns`: the fields of the charts that draw it, the value of the stats
/// that read it and the option columns of the filters that use it.
pub fn columns(loaded: &LoadedBoard, query: &str, columns: &HashSet<&str>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut have: Vec<&str> = columns.iter().copied().collect();
    have.sort_unstable();
    let have = have.join(", ");
    let board = &loaded.board;
    let unknown = |pointer: &str, rule: &'static str, message: String, fix: String| {
        Diagnostic::new(&loaded.file, loaded.doc.line(pointer), rule, message, fix)
    };

    for (index, chart) in board.charts.iter().enumerate() {
        if chart.query != query {
            continue;
        }
        let value = Value::Object(chart.echarts.clone());
        let fields = echarts::collect(&value, &format!("/charts/{index}/echarts"));
        for (pointer, field) in &fields.used {
            if !columns.contains(field.as_str()) {
                diagnostics.push(unknown(
                    pointer,
                    "chart-unknown-field",
                    format!(
                        "the chart `{}` names the column `{field}`, which the query `{query}` does not return",
                        chart.title
                    ),
                    format!("use one of the columns: {have}"),
                ));
            }
        }
    }
    for (index, stat) in board.stats.iter().enumerate() {
        if stat.query == query && !columns.contains(stat.value.as_str()) {
            diagnostics.push(unknown(
                &format!("/stats/{index}/value"),
                "stat-unknown-column",
                format!(
                    "the stat `{}` reads the column `{}`, which the query `{query}` does not return",
                    stat.label, stat.value
                ),
                format!("use one of the columns: {have}"),
            ));
        }
    }
    for (index, stat) in board.stats.iter().enumerate() {
        let Some(trend) = stat.trend.as_ref().filter(|t| t.query == query) else {
            continue;
        };
        for (key, column) in [("x", &trend.x), ("value", &trend.value)] {
            if !columns.contains(column.as_str()) {
                diagnostics.push(unknown(
                    &format!("/stats/{index}/trend/{key}"),
                    "stat-unknown-column",
                    format!(
                        "the trend of the stat `{}` reads the column `{column}`, which the query `{query}` does not return",
                        stat.label
                    ),
                    format!("use one of the columns: {have}"),
                ));
            }
        }
    }
    for (index, filter) in board.filters.iter().enumerate() {
        let Some(from) = &filter.options_from else {
            continue;
        };
        if from.query != query {
            continue;
        }
        for key in ["value", "label"] {
            let column = if key == "value" {
                &from.value
            } else {
                &from.label
            };
            if !columns.contains(column.as_str()) {
                diagnostics.push(unknown(
                    &format!("/filters/{index}/options_from/{key}"),
                    "filter-unknown-column",
                    format!(
                        "the filter `{}` reads the column `{column}`, which the query `{query}` does not return",
                        filter.label
                    ),
                    format!("use one of the columns: {have}"),
                ));
            }
        }
    }
    diagnostics
}
