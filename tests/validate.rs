//! Offline validation, over the fixtures in tests/fixtures/invalid: each file
//! holds the mistakes named in its file name, and the expected diagnostics
//! are pinned here with their lines.

// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use enlightenviz::catalog::load_dir;

fn found(dir: &str) -> Vec<(String, Option<usize>, &'static str)> {
    let (_, diagnostics) = load_dir(Path::new(dir));
    let mut found: Vec<_> = diagnostics
        .into_iter()
        .map(|d| {
            let name = Path::new(&d.file)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, d.line, d.rule)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn every_mistake_is_reported_with_its_file_line_and_rule() {
    let expected: Vec<(&str, Option<usize>, &str)> = vec![
        ("chart_unknown_query.yml", Some(11), "chart-unknown-query"),
        ("duplicate_b.yml", Some(2), "board-duplicate"),
        ("name_invalid.yml", Some(1), "name-invalid"),
        ("name_invalid.yml", Some(7), "name-invalid"),
        ("name_invalid.yml", Some(7), "sql-unused-param"),
        ("param_declarations.yml", Some(9), "param-default-invalid"),
        (
            "param_declarations.yml",
            Some(12),
            "param-max-items-invalid",
        ),
        (
            "param_declarations.yml",
            Some(15),
            "param-max-items-invalid",
        ),
        ("schema_bad_param_type.yml", Some(8), "schema"),
        ("schema_missing_field.yml", Some(4), "schema"),
        ("schema_no_charts.yml", Some(6), "board-no-charts"),
        ("schema_unknown_field.yml", Some(3), "schema"),
        ("sql_parameters.yml", Some(5), "sql-undeclared-param"),
        ("sql_parameters.yml", Some(11), "sql-unused-param"),
        ("sql_parameters.yml", Some(13), "sql-positional-param"),
        ("sql_shape.yml", Some(5), "sql-unterminated"),
        ("sql_shape.yml", Some(7), "sql-multiple-statements"),
        ("sql_shape.yml", Some(11), "sql-empty"),
        ("yaml_anchor.yml", Some(4), "yaml-unsupported"),
        ("yaml_syntax.yml", Some(4), "yaml-syntax"),
    ];
    let expected: Vec<_> = expected
        .into_iter()
        .map(|(f, l, r)| (f.to_string(), l, r))
        .collect();
    assert_eq!(found("tests/fixtures/invalid"), expected);
}

#[test]
fn the_messages_say_what_to_change() {
    let (_, diagnostics) = load_dir(Path::new("tests/fixtures/invalid"));
    for d in &diagnostics {
        assert!(!d.message.is_empty() && !d.fix.is_empty(), "{d}");
    }
    let by = |file: &str, rule: &str| {
        diagnostics
            .iter()
            .find(|d| d.file.ends_with(file) && d.rule == rule)
            .unwrap_or_else(|| panic!("no {rule} in {file}"))
            .clone()
    };
    let unknown = by("schema_unknown_field.yml", "schema");
    assert!(
        unknown.message.contains("unknown field `refresh`"),
        "{unknown}"
    );
    let query = by("chart_unknown_query.yml", "chart-unknown-query");
    assert!(
        query.message.contains("`b`") && query.fix.contains('a'),
        "{query}"
    );
    let sql = by("sql_parameters.yml", "sql-undeclared-param");
    assert!(
        sql.message.contains("$missing") && sql.message.contains("line 2 of the query"),
        "{sql}"
    );
}

#[test]
fn the_examples_and_the_api_fixtures_are_valid() {
    for dir in [
        "examples/earthquakes",
        "tests/fixtures/api",
        "tests/fixtures/writes",
    ] {
        let (catalog, diagnostics) = load_dir(Path::new(dir));
        assert!(diagnostics.is_empty(), "{dir}: {diagnostics:?}");
        assert!(!catalog.boards.is_empty());
    }
}

#[test]
fn a_directory_that_is_missing_or_empty_is_an_error() {
    let (_, missing) = load_dir(Path::new("tests/fixtures/does-not-exist"));
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].rule, "boards-dir-unreadable");
    let (_, empty) = load_dir(Path::new("tests/fixtures/empty"));
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].rule, "boards-dir-unreadable");
}

#[test]
fn a_directory_without_boards_is_an_error() {
    // examples/ holds only a directory, no board file of its own.
    let (_, diagnostics) = load_dir(Path::new("examples"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule, "boards-dir-empty");
}
