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
        ("chart_no_spec.yml", Some(7), "schema"),
        ("chart_spec.yml", Some(10), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(12), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(16), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(18), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(19), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(24), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(24), "chart-spec-forbidden"),
        ("chart_spec.yml", Some(30), "chart-spec-forbidden"),
        ("chart_unknown_query.yml", Some(11), "chart-unknown-query"),
        ("duplicate_b.yml", Some(2), "board-duplicate"),
        ("filters.yml", Some(4), "filter-unknown-param"),
        ("filters.yml", Some(6), "filter-duplicate"),
        ("filters.yml", Some(8), "filter-control-type"),
        ("filters.yml", Some(9), "filter-options-invalid"),
        ("filters.yml", Some(9), "filter-options-invalid"),
        ("filters.yml", Some(10), "filter-options-invalid"),
        ("filters.yml", Some(11), "filter-options-invalid"),
        ("filters.yml", Some(12), "filter-options-invalid"),
        ("filters.yml", Some(13), "filter-options-invalid"),
        ("filters.yml", Some(14), "filter-options-invalid"),
        ("filters.yml", Some(15), "filter-options-invalid"),
        ("filters.yml", Some(16), "filter-control-type"),
        ("filters.yml", Some(18), "filter-options-invalid"),
        ("filters.yml", Some(18), "filter-options-invalid"),
        ("filters.yml", Some(35), "filter-param-required"),
        ("filters.yml", Some(39), "filter-param-conflict"),
        ("filters.yml", Some(39), "filter-param-conflict"),
        ("filters.yml", Some(40), "filter-param-conflict"),
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
        ("stats.yml", Some(5), "stat-unknown-query"),
        ("stats.yml", Some(6), "stat-hero-multiple"),
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
fn the_new_messages_name_the_cause() {
    let (_, diagnostics) = load_dir(Path::new("tests/fixtures/invalid"));
    let message = |file: &str, rule: &str, line: usize| {
        diagnostics
            .iter()
            .find(|d| d.file.ends_with(file) && d.rule == rule && d.line == Some(line))
            .unwrap_or_else(|| panic!("no {rule} at {file}:{line}"))
            .message
            .clone()
    };
    assert!(message("chart_spec.yml", "chart-spec-forbidden", 12).contains("the key `data`"));
    assert!(message("chart_spec.yml", "chart-spec-forbidden", 16).contains("a link"));
    assert!(
        message("filters.yml", "filter-param-conflict", 39)
            .contains("`date` in `a` and `text` in `b`")
    );
    assert!(message("filters.yml", "filter-param-conflict", 40).contains("different defaults"));
    assert!(message("filters.yml", "filter-control-type", 16).contains("`select` control"));
    assert!(message("stats.yml", "stat-hero-multiple", 6).contains("stat number 1"));
}

#[test]
fn the_example_and_the_api_fixtures_are_valid() {
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

#[test]
fn a_huge_number_of_sql_errors_is_reported_quickly_capped_and_said_so() {
    // A 900 KB query that is one undeclared name 300,000 times.
    let dir = std::env::temp_dir().join(format!("enlightenviz-many-errors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sql = format!("select {}", "$a ".repeat(300_000));
    let board = format!(
        "board: many\ntitle: Many errors\nqueries:\n  q:\n    sql: |\n      {sql}\ncharts:\n  - title: C\n    query: q\n    echarts: {{series: []}}\n"
    );
    std::fs::write(dir.join("many.yml"), board).unwrap();

    let started = std::time::Instant::now();
    let (_, diagnostics) = load_dir(&dir);
    let elapsed = started.elapsed();
    std::fs::remove_dir_all(&dir).unwrap();

    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "took {elapsed:?}"
    );
    let undeclared = diagnostics
        .iter()
        .filter(|d| d.rule == "sql-undeclared-param")
        .count();
    assert_eq!(undeclared, enlightenviz::sql::MAX_ERRORS);
    let truncated: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.rule == "sql-errors-truncated")
        .collect();
    assert_eq!(truncated.len(), 1, "the cut must be reported once");
    assert!(
        truncated[0].message.contains("299950 more problems"),
        "{}",
        truncated[0]
    );
    assert_eq!(diagnostics.len(), enlightenviz::sql::MAX_ERRORS + 1);
}
