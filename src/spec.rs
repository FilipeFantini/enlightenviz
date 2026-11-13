//! What an ECharts option may contain, and what it may not.
//!
//! An option is the author's, and ECharts has more places that take markup,
//! data or a link than a list of keys could ever name. So where it is cheap the
//! rules say what is allowed rather than what is refused:
//!
//! - a series is a `bar`, a `line` or a `scatter`, the kinds the house theme
//!   styles and the tests draw, and an option has at least one series; series
//!   live at the top level, so `baseOption`, `media` and `timeline`, which hide
//!   a second list of them, are refused there;
//! - a `tooltip` object holds only `show` and `trigger`: ECharts renders a
//!   tooltip's `formatter` as HTML, and the page owns the tooltip;
//! - there is no `toolbox`.
//!
//! What is refused by name is the short list of keys that carry data or make
//! a link: `data`, `datasets`, `dataset`, `url`, `href`, `link`, `sublink`,
//! `target` and `image`. A string that starts `http://`, `https://`, `//` or
//! `image://` is refused as an early, helpful error: it is not a URL filter,
//! the content security policy is what stops a load, and the refused keys are
//! what stop a navigation. The one static dataset is the world outline, which
//! the page provides under the reserved name `world`: an option says
//! `map: world`.
//!
//! What this does not check is in `docs/boards.md`.

use serde_json::Value;

use crate::source::child;

/// The series types the page styles and the tests exercise.
pub const SERIES_TYPES: [&str; 3] = ["bar", "line", "scatter"];

/// Keys that carry data, whatever the series.
const DATA_KEYS: [&str; 5] = ["data", "datasets", "dataset", "url", "href"];

/// Keys that make a link or load an image, wherever they appear.
const LINK_KEYS: [&str; 4] = ["link", "sublink", "target", "image"];

/// Keys that hide a second list of series, refused at the top level.
const HIDDEN_SERIES_KEYS: [&str; 3] = ["baseOption", "media", "timeline"];

/// What a `tooltip` object may hold.
const TOOLTIP_KEYS: [&str; 2] = ["show", "trigger"];

/// String values that are a link, whatever key holds them. `image://` is how
/// ECharts writes a symbol that is loaded from a URL.
const LINK_PREFIXES: [&str; 4] = ["http://", "https://", "//", "image://"];

/// One thing in an option that is not allowed.
#[derive(Debug, PartialEq, Eq)]
pub struct Problem {
    /// The JSON Pointer of the key, or of the value for a link.
    pub pointer: String,
    pub rule: &'static str,
    /// What is wrong, as the end of a sentence that starts with the chart.
    pub what: String,
    pub fix: String,
}

fn forbidden(pointer: String, what: String, fix: &str) -> Problem {
    Problem {
        pointer,
        rule: "chart-spec-forbidden",
        what,
        fix: fix.to_string(),
    }
}

/// Every key or link in `spec`, which sits at `pointer`, that is not allowed.
pub fn forbidden_in(spec: &Value, pointer: &str) -> Vec<Problem> {
    let mut found = Vec::new();
    if let Value::Object(map) = spec {
        for key in HIDDEN_SERIES_KEYS.iter().filter(|k| map.contains_key(**k)) {
            found.push(forbidden(
                child(pointer, key),
                format!("has the key `{key}`, which would hide a second list of series"),
                "put the series at the top level of the option",
            ));
        }
    }
    walk(spec, pointer, &mut found);
    found
}

fn walk(value: &Value, pointer: &str, found: &mut Vec<Problem>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let here = child(pointer, key);
                let key = key.as_str();
                if DATA_KEYS.contains(&key) {
                    found.push(forbidden(
                        here,
                        format!("has the key `{key}`, and an option carries no data"),
                        "remove it; the page adds the rows of the chart's query. The one static dataset is the world outline: `map: world`",
                    ));
                } else if LINK_KEYS.contains(&key) {
                    found.push(forbidden(
                        here,
                        format!("has the key `{key}`, and an option names no link or image"),
                        "remove it",
                    ));
                } else if key == "toolbox" {
                    found.push(forbidden(
                        here,
                        "has a `toolbox`, and the page has none".to_string(),
                        "remove it",
                    ));
                } else if key == "tooltip" {
                    tooltip(item, &here, found);
                } else if key == "encode" && item.is_object() {
                    // The keys of `encode` are roles (`x`, `y`, `tooltip`...), not
                    // options; only the column names under them are read.
                    for (role, columns) in item.as_object().into_iter().flatten() {
                        walk(columns, &child(&here, role), found);
                    }
                } else {
                    walk(item, &here, found);
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &child(pointer, &index.to_string()), found);
            }
        }
        Value::String(text) => {
            let lower = text.trim_start().to_ascii_lowercase();
            if LINK_PREFIXES.iter().any(|p| lower.starts_with(p)) {
                found.push(forbidden(
                    pointer.to_string(),
                    "has a string that starts like a link, and an option names no link or image"
                        .to_string(),
                    "remove it",
                ));
            }
        }
        _ => {}
    }
}

/// A `tooltip` holds `show` and `trigger` and nothing else. Its other keys are
/// not walked: they are already an error.
fn tooltip(value: &Value, pointer: &str, found: &mut Vec<Problem>) {
    let Value::Object(map) = value else {
        found.push(forbidden(
            pointer.to_string(),
            "has a `tooltip` that is not an object with `show` and `trigger`".to_string(),
            "write `tooltip: {trigger: item}`",
        ));
        return;
    };
    for key in map.keys().filter(|k| !TOOLTIP_KEYS.contains(&k.as_str())) {
        found.push(forbidden(
            child(pointer, key),
            format!("has the tooltip key `{key}`; the page owns the tooltip, and only `show` and `trigger` may be set"),
            "remove it; the page writes the tooltip, value first",
        ));
    }
}

/// The series of `spec`: at least one, each of an allowed type.
pub fn series_problems(spec: &Value, pointer: &str) -> Vec<Problem> {
    let allowed = SERIES_TYPES.join(", ");
    let bad = |at: String, what: String| Problem {
        pointer: at,
        rule: "chart-series-type",
        what,
        fix: format!("use `series` with at least one series, each of type {allowed}"),
    };
    let series = spec.get("series");
    let Some(Value::Array(list)) = series.filter(|s| s.as_array().is_some_and(|l| !l.is_empty()))
    else {
        return vec![bad(
            child(pointer, "series"),
            "has no series: `series` must be a list with at least one series".to_string(),
        )];
    };
    let mut found = Vec::new();
    for (index, item) in list.iter().enumerate() {
        let at = child(&child(pointer, "series"), &index.to_string());
        match item.get("type").and_then(Value::as_str) {
            Some(kind) if SERIES_TYPES.contains(&kind) => {}
            Some(kind) => found.push(bad(
                child(&at, "type"),
                format!("has a series of type `{kind}`, which the page does not draw (allowed: {allowed})"),
            )),
            None => found.push(bad(
                at,
                format!("has a series with no `type` (allowed: {allowed})"),
            )),
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn keys(spec: &Value) -> Vec<String> {
        forbidden_in(spec, "/s")
            .into_iter()
            .map(|f| f.pointer)
            .collect()
    }

    #[test]
    fn data_links_and_images_are_refused_by_key_and_by_prefix() {
        let spec = json!({
            "dataset": {"source": []},
            "title": {"text": "t", "link": "https://example.com/", "sublink": "x", "target": "self"},
            "series": [
                {"symbol": "image://a.png", "data": [1]},
                {"symbol": "circle", "url": "x", "href": "y", "image": "z"},
                {"symbol": "//cdn.example.com/a.png"}
            ]
        });
        let mut found = keys(&spec);
        found.sort();
        assert_eq!(
            found,
            [
                "/s/dataset",
                "/s/series/0/data",
                "/s/series/0/symbol",
                "/s/series/1/href",
                "/s/series/1/image",
                "/s/series/1/url",
                "/s/series/2/symbol",
                "/s/title/link",
                "/s/title/sublink",
                "/s/title/target"
            ]
        );
    }

    #[test]
    fn a_tooltip_holds_show_and_trigger_only_wherever_it_is() {
        let spec = json!({
            "tooltip": {"trigger": "item", "show": true},
            "series": [{"type": "bar", "tooltip": {"formatter": "<img src=x onerror=1>", "show": true}}],
            "xAxis": {"axisPointer": {"label": {"formatter": "t"}}, "tooltip": {"extraCssText": "x"}},
            "yAxis": {"tooltip": 3}
        });
        let mut found = keys(&spec);
        found.sort();
        assert_eq!(
            found,
            [
                "/s/series/0/tooltip/formatter",
                "/s/xAxis/tooltip/extraCssText",
                "/s/yAxis/tooltip"
            ]
        );
    }

    #[test]
    fn there_is_no_toolbox_and_series_do_not_hide_in_other_keys() {
        let spec = json!({"toolbox": {"feature": {"dataView": {"title": "<b>"}}}, "timeline": {}, "media": [], "baseOption": {}});
        let mut found = keys(&spec);
        found.sort();
        assert_eq!(
            found,
            ["/s/baseOption", "/s/media", "/s/timeline", "/s/toolbox"]
        );
        // Only at the top level: a key of that name deeper down is not a hiding place.
        assert!(
            keys(&json!({"series": [{"type": "bar", "name": "media"}], "legend": {"data2": 1}}))
                .is_empty()
        );
    }

    #[test]
    fn ordinary_text_and_the_world_map_are_fine() {
        let spec =
            json!({"title": "see https://x for more", "note": "a // b", "geo": {"map": "world"}});
        assert!(keys(&spec).is_empty());
    }

    #[test]
    fn the_roles_of_encode_are_not_options() {
        // `tooltip` is a role of `encode` (the columns a tooltip shows), not a tooltip.
        let roles = json!({"series": [{"encode": {"tooltip": ["a"], "link": ["b"], "x": "c"}}]});
        assert!(keys(&roles).is_empty());
        // The column names under a role are still read.
        let link = json!({"series": [{"encode": {"x": "https://x"}}]});
        assert_eq!(keys(&link), ["/s/series/0/encode/x"]);
    }

    #[test]
    fn a_series_is_a_bar_a_line_or_a_scatter_and_there_is_one() {
        let ok = json!({"series": [{"type": "bar"}, {"type": "line"}, {"type": "scatter"}]});
        assert!(series_problems(&ok, "/s").is_empty());
        let rules = |spec: Value| -> Vec<String> {
            series_problems(&spec, "/s")
                .into_iter()
                .map(|p| p.pointer)
                .collect()
        };
        assert_eq!(rules(json!({})), ["/s/series"]);
        assert_eq!(rules(json!({"series": []})), ["/s/series"]);
        assert_eq!(rules(json!({"series": {"type": "bar"}})), ["/s/series"]);
        assert_eq!(
            rules(
                json!({"series": [{"type": "graph", "nodes": []}, {"name": "x"}, {"type": "bar"}]})
            ),
            ["/s/series/0/type", "/s/series/1"]
        );
    }
}
