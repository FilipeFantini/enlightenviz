//! The board format: the structs a YAML file is read into, and the JSON Schema
//! generated from the same structs, so the two cannot drift apart.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::param::ParamType;

/// Whether `name` is a valid id of a board or name of a query
/// (`^[A-Za-z_][A-Za-z0-9_-]{0,62}$`, the pattern in the schema), or, with
/// `dash` false, of a parameter (`^[A-Za-z_][A-Za-z0-9_]{0,62}$`). Names appear
/// in URLs and in SQL, so the set is small.
pub fn valid_name(name: &str, dash: bool) -> bool {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    first_ok
        && name.len() <= 63
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || (dash && c == '-'))
}

/// A dashboard: named queries, and charts that draw their results. One board
/// is one YAML file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "enlightenviz board")]
pub struct Board {
    /// The id of the board; part of the URL of its queries. Unique across the
    /// boards directory. Letters, digits, `_` and `-`, starting with a letter
    /// or `_`, at most 63 characters.
    #[schemars(regex(pattern = r"^[A-Za-z_][A-Za-z0-9_-]{0,62}$"))]
    pub board: String,
    /// The title shown above the board.
    pub title: String,
    /// Optional text shown under the title.
    #[serde(default)]
    pub text: Option<String>,
    /// The queries of the board, by name. The name is part of the URL, and has
    /// the same form as the id of a board.
    #[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z_][A-Za-z0-9_-]{0,62}$"}))]
    pub queries: BTreeMap<String, Query>,
    /// The charts of the board, in the order they are shown. At least one.
    #[schemars(length(min = 1))]
    pub charts: Vec<Chart>,
}

/// One SQL query and the typed parameters it accepts.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// One SQL statement, a `SELECT` or a `WITH ... SELECT`. Write a parameter
    /// as `$name`; every parameter declared in `params` must be used, and
    /// every `$name` must be declared. Do not write `$1`. A single trailing
    /// `;` is allowed.
    pub sql: String,
    /// The parameters of the query, by name. A name has the form
    /// `[A-Za-z_][A-Za-z0-9_]*`, at most 63 characters.
    #[serde(default)]
    #[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z_][A-Za-z0-9_]{0,62}$"}))]
    pub params: BTreeMap<String, Param>,
}

/// The declaration of one parameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// The type of the value.
    #[serde(rename = "type")]
    pub ty: ParamType,
    /// The value used when a request gives none, of the declared type: a
    /// quoted string for `text` and `date`, a number for `integer` and
    /// `number`, `true` or `false` for `boolean`, and a YAML list of those for
    /// the list types. A parameter without a default is required.
    #[serde(default)]
    pub default: Option<Value>,
    /// For list types only: the most items a request may give. Default 100,
    /// at most 1000.
    #[serde(default)]
    #[schemars(range(min = 1, max = 1000))]
    pub max_items: Option<usize>,
}

/// A chart: a Vega-Lite specification that draws the result of a query.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    /// The title of the chart.
    pub title: String,
    /// The name of the query this chart draws; a key of `queries`.
    pub query: String,
    /// A Vega-Lite specification, kept as it is. `validate` checks only that
    /// every `field` it names is a column of the query, or is created by a
    /// transform's `as`.
    pub vega_lite: Map<String, Value>,
}

/// The JSON Schema of a board, pretty-printed, with a trailing newline.
pub fn schema_json() -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(&schemars::schema_for!(Board))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn names() {
        for good in ["a", "_a", "earthquakes", "by-day", "A1_b-2"] {
            assert!(valid_name(good, true), "{good}");
        }
        assert!(!valid_name("by-day", false));
        assert!(valid_name("by_day", false));
        let longest = "a".repeat(63);
        assert!(valid_name(&longest, true));
        assert!(!valid_name(&format!("{longest}a"), true));
        for bad in ["", "1a", "-a", "a b", "a/b", "a.b", "é", "a$", "a'b"] {
            assert!(!valid_name(bad, true), "{bad}");
        }
    }
}
