//! The command line, run as a program. None of these needs a database.

// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::{Command, Output};

fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_enlightenviz"));
    command
        .args(args)
        .env_remove("ENLIGHTENVIZ_DATABASE_URL")
        .env_remove("ENLIGHTENVIZ_DATABASE_URL_FILE")
        .env_remove("ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn schema_prints_the_committed_schema() {
    let out = run(&["schema"], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text(&out.stdout),
        include_str!("../schema/board.schema.json")
    );
}

#[test]
fn validate_passes_a_good_board_offline() {
    let out = run(&["validate", "examples/earthquakes"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stdout));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("ok: 1 board(s) valid"), "{stdout}");
    assert!(stdout.contains("no database is configured"), "{stdout}");
}

#[test]
fn validate_fails_with_exit_1_and_structured_json() {
    let out = run(
        &["validate", "tests/fixtures/invalid", "--format", "json"],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["database_checked"], false);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 62);
    for d in diagnostics {
        for key in ["file", "line", "rule", "message", "fix"] {
            assert!(d.get(key).is_some(), "{d} has no {key}");
        }
    }
}

#[test]
fn validate_a_missing_directory_is_an_error() {
    let out = run(&["validate", "tests/fixtures/nowhere"], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stdout).contains("boards-dir-unreadable"));
}

#[test]
fn a_database_without_the_unencrypted_option_is_refused() {
    let url = [("ENLIGHTENVIZ_DATABASE_URL", "postgres://u:p@127.0.0.1:1/db")];
    for args in [
        vec!["validate", "examples/earthquakes"],
        vec!["serve", "--boards", "examples/earthquakes"],
    ] {
        let out = run(&args, &url);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("without TLS") && stderr.contains("--allow-unencrypted-database"),
            "{stderr}"
        );
        // The URL, with its password, is never printed.
        assert!(!stderr.contains("u:p@"), "{stderr}");
    }
}

#[test]
fn serve_needs_a_database_and_only_one_source_for_its_url() {
    let out = run(&["serve", "--boards", "examples/earthquakes"], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("no database is configured"));

    let out = run(
        &[
            "serve",
            "--boards",
            "examples/earthquakes",
            "--database-url-file",
            "Cargo.toml",
        ],
        &[("ENLIGHTENVIZ_DATABASE_URL", "postgres://u:p@127.0.0.1:1/db")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("use one"));
}

#[test]
fn serve_refuses_values_outside_the_limits() {
    for args in [
        ["--pool-size", "0"],
        ["--pool-size", "65"],
        ["--row-limit", "0"],
        ["--statement-timeout-ms", "0"],
        ["--acquire-timeout-ms", "0"],
        ["--listen", "not-an-address"],
    ] {
        let mut full = vec!["serve", "--boards", "examples/earthquakes"];
        full.extend(args);
        let out = run(&full, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
