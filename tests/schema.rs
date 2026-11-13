//! The committed JSON Schema must be the one the code generates.

// Test code: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use enlightenviz::board::schema_json;

#[test]
fn the_committed_schema_is_the_generated_one() {
    let generated = schema_json().unwrap();
    let committed = include_str!("../schema/board.schema.json");
    assert!(
        generated == committed,
        "schema/board.schema.json is out of date: regenerate it with\n  enlightenviz schema > schema/board.schema.json\nand commit the result"
    );
}
