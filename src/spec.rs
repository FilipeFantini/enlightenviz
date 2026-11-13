//! What a chart specification may not carry: data, and anything that makes
//! the browser fetch from somewhere.
//!
//! The page injects the rows of the chart's query, so a specification that
//! brings its own data would draw something the query does not say, and one
//! that names a URL would make the page request it. Both are errors at
//! `validate`. The one static dataset is the world outline, which the page
//! provides under the reserved name `world`: an option says `map: world`.

use serde_json::Value;

use crate::source::child;

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
pub fn forbidden(spec: &Value, pointer: &str) -> Vec<Forbidden> {
    let mut found = Vec::new();
    walk(spec, pointer, &mut found);
    found
}

fn walk(value: &Value, pointer: &str, found: &mut Vec<Forbidden>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let here = child(pointer, key);
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    found.push(Forbidden {
                        pointer: here,
                        what: format!("the key `{key}`"),
                    });
                    continue;
                }
                walk(item, &here, found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &child(pointer, &index.to_string()), found);
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

    fn keys(spec: &Value) -> Vec<String> {
        forbidden(spec, "/s")
            .into_iter()
            .map(|f| f.pointer)
            .collect()
    }

    #[test]
    fn data_datasets_urls_and_links_are_refused() {
        let spec = json!({
            "dataset": {"source": []},
            "title": {"text": "t", "link": "https://example.com/"},
            "series": [
                {"symbol": "image://a.png", "data": [1]},
                {"symbol": "circle", "url": "x", "href": "y"},
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
                "/s/series/1/url",
                "/s/series/2/symbol",
                "/s/title/link"
            ]
        );
    }

    #[test]
    fn a_world_dataset_is_refused_and_the_world_map_is_not() {
        assert_eq!(
            keys(&json!({"series": [{"data": {"name": "world"}}]})),
            ["/s/series/0/data"]
        );
        assert!(keys(&json!({"geo": {"map": "world"}})).is_empty());
    }

    #[test]
    fn ordinary_text_is_fine() {
        let spec = json!({"title": "see https://x for more", "note": "a // b", "map": "world"});
        assert!(keys(&spec).is_empty());
    }
}
