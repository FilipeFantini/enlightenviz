//! What a chart specification may not carry: data, and anything that makes
//! the browser fetch from somewhere.
//!
//! The page injects the rows of the chart's query, so a specification that
//! brings its own data would draw something the query does not say, and one
//! that names a URL would make the page request it. Both are errors at
//! `validate`, whichever library the specification is for. The one static
//! dataset is the world outline, which the page provides under the reserved
//! name `world`: a Vega-Lite layer may say `data: {name: world}`, and an
//! ECharts option says `map: world`.

use serde_json::Value;

use crate::board::Engine;
use crate::source::child;

/// The reserved name of the world outline.
pub const WORLD: &str = "world";

/// Keys that carry data or a link, wherever they appear.
const FORBIDDEN_KEYS: [&str; 5] = ["data", "datasets", "dataset", "url", "href"];

/// String values that are a link, whatever key holds them. `image://` is how
/// ECharts writes a symbol that is loaded from a URL.
const FORBIDDEN_PREFIXES: [&str; 4] = ["http://", "https://", "//", "image://"];

/// One thing in a specification that is not allowed.
#[derive(Debug, PartialEq, Eq)]
pub struct Forbidden {
    /// The JSON Pointer of the key, or of the value for a link.
    pub pointer: String,
    pub what: String,
}

/// Every forbidden key or link in `spec`, which sits at `pointer`.
pub fn forbidden(engine: Engine, spec: &Value, pointer: &str) -> Vec<Forbidden> {
    let mut found = Vec::new();
    walk(engine, spec, pointer, &mut found);
    found
}

fn is_world_reference(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == 1 && o.get("name").and_then(Value::as_str) == Some(WORLD))
}

fn walk(engine: Engine, value: &Value, pointer: &str, found: &mut Vec<Forbidden>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let here = child(pointer, key);
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    if engine == Engine::VegaLite && key == "data" && is_world_reference(item) {
                        continue;
                    }
                    found.push(Forbidden {
                        pointer: here,
                        what: format!("the key `{key}`"),
                    });
                    continue;
                }
                walk(engine, item, &here, found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(engine, item, &child(pointer, &index.to_string()), found);
            }
        }
        Value::String(text) => {
            let lower = text.trim_start().to_ascii_lowercase();
            if FORBIDDEN_PREFIXES.iter().any(|p| lower.starts_with(p)) {
                found.push(Forbidden {
                    pointer: pointer.to_string(),
                    what: "a link".to_string(),
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn keys(engine: Engine, spec: &Value) -> Vec<String> {
        forbidden(engine, spec, "/s")
            .into_iter()
            .map(|f| f.pointer)
            .collect()
    }

    #[test]
    fn data_datasets_urls_and_links_are_refused() {
        let spec = json!({
            "data": {"values": [{"a": 1}]},
            "datasets": {"x": []},
            "dataset": {"source": []},
            "layer": [{"encoding": {"url": {"field": "u"}, "href": {"field": "h"}}}],
            "title": {"text": "t", "link": "https://example.com/"},
            "series": [{"symbol": "image://a.png"}, {"symbol": "circle"}]
        });
        let mut found = keys(Engine::VegaLite, &spec);
        found.sort();
        assert_eq!(
            found,
            [
                "/s/data",
                "/s/dataset",
                "/s/datasets",
                "/s/layer/0/encoding/href",
                "/s/layer/0/encoding/url",
                "/s/series/0/symbol",
                "/s/title/link"
            ]
        );
    }

    #[test]
    fn the_world_outline_is_the_one_allowed_dataset_and_only_in_vega_lite() {
        let spec = json!({"layer": [{"data": {"name": "world"}, "mark": "geoshape"}]});
        assert!(keys(Engine::VegaLite, &spec).is_empty());
        assert_eq!(keys(Engine::Echarts, &spec), ["/s/layer/0/data"]);
        for other in [
            json!({"data": {"name": "rows"}}),
            json!({"data": {"name": "world", "url": "x"}}),
            json!({"data": {"name": "world", "format": {}}}),
            json!({"data": "world"}),
        ] {
            assert!(!keys(Engine::VegaLite, &other).is_empty(), "{other}");
        }
    }

    #[test]
    fn ordinary_text_and_columns_named_like_keys_are_fine() {
        let spec = json!({"encoding": {"x": {"field": "url"}}, "title": "see https://x for more",
                          "note": "a // b", "map": "world"});
        assert!(keys(Engine::Echarts, &spec).is_empty());
    }
}
