//! The board format: the structs a YAML file is read into, and the JSON Schema
//! generated from the same structs, so the two cannot drift apart.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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

/// A dashboard: named queries, filters, figures and charts that draw their
/// results. One board is one YAML file.
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
    /// The filters shown above the board, in order. Each one drives the
    /// parameter of its name in every query that declares it.
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// The figures shown above the charts: one value of one query each.
    #[serde(default)]
    pub stats: Vec<Stat>,
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

/// How a filter is drawn and what it lets the visitor choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    /// One value out of the options; for a scalar parameter.
    Select,
    /// Any number of the options, at least one; for a list parameter.
    Multiselect,
    /// A calendar date; for a `date` parameter.
    Date,
}

/// A control above the board that sets one parameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    /// The label shown with the control.
    pub label: String,
    /// The parameter this filter sets, in every query that declares it.
    pub param: String,
    /// How the filter is drawn. It must fit the type of the parameter:
    /// `select` for text, integer, number or boolean, `multiselect` for a
    /// list, `date` for a date.
    pub control: Control,
    /// For `select` and `multiselect`: the options as a fixed list. Give this
    /// or `options_from`, not both. A `date` filter has neither.
    #[serde(default)]
    pub options: Option<Vec<FilterOption>>,
    /// For `select` and `multiselect`: take the options from a query of this
    /// board, run once without parameters.
    #[serde(default)]
    pub options_from: Option<OptionsFrom>,
}

/// One fixed option of a filter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterOption {
    /// The value sent as the parameter: a quoted string for text and dates, a
    /// number for integer and number, `true` or `false` for boolean.
    pub value: Value,
    /// The text shown to the visitor.
    pub label: String,
}

/// Where the options of a filter come from.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionsFrom {
    /// A query of this board. It is run without parameters, so every
    /// parameter it declares needs a default.
    pub query: String,
    /// The column that holds the value sent as the parameter.
    pub value: String,
    /// The column that holds the text shown to the visitor.
    pub label: String,
}

/// How a stat's number is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatFormat {
    /// A whole number with thousands separators: 12,345.
    Integer,
    /// One decimal: 5.4.
    OneDecimal,
    /// A short form for large numbers: 12.3K, 4.5M.
    Compact,
}

/// A figure shown above the charts: one column of the single row a query
/// returns.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stat {
    /// The label shown with the number.
    pub label: String,
    /// The name of the query; a key of `queries`. It must return one row.
    pub query: String,
    /// The column that holds the number.
    pub value: String,
    /// How the number is written. Without it, up to two decimals.
    #[serde(default)]
    pub format: Option<StatFormat>,
    /// The headline figure of the board, drawn larger. At most one stat.
    #[serde(default)]
    pub hero: bool,
}

/// Which colours a chart takes its series from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    /// The eight categorical colours, in their fixed order (the default).
    Categorical,
    /// The four-step blue ramp, weakest to strongest, for classes that have
    /// an order.
    Ordered,
}

/// The chart library that draws a chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Engine {
    #[serde(rename = "vega_lite")]
    VegaLite,
    #[serde(rename = "echarts")]
    Echarts,
}

/// A chart: a specification, in the key of the library that draws it, that
/// draws the result of a query. The specification carries no data: the page
/// adds the rows of the query.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    /// The title of the chart.
    pub title: String,
    /// The name of the query this chart draws; a key of `queries`.
    pub query: String,
    /// A Vega-Lite specification, kept as it is. Give this or `echarts`.
    #[serde(default)]
    pub vega_lite: Option<Map<String, Value>>,
    /// An Apache ECharts option, kept as it is. Give this or `vega_lite`.
    #[serde(default)]
    pub echarts: Option<Map<String, Value>>,
    /// The colours the series take: `categorical` (default) or `ordered`.
    #[serde(default)]
    pub palette: Option<Palette>,
}

impl Chart {
    /// The specification and the library that draws it, when exactly one of
    /// the two keys is present.
    pub fn spec(&self) -> Option<(Engine, &Map<String, Value>)> {
        match (&self.vega_lite, &self.echarts) {
            (Some(spec), None) => Some((Engine::VegaLite, spec)),
            (None, Some(spec)) => Some((Engine::Echarts, spec)),
            _ => None,
        }
    }
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
