//! What `validate` can know about a Vega-Lite specification without running
//! it: which data fields it draws, and which fields its transforms create.
//!
//! The specification itself stays opaque; this reads two things from it.
//! Every `"field": "<string>"` anywhere in it is a field the chart draws. Every
//! string under an `"as"` key (a string, or a list of strings) is a field a
//! transform creates, so it is known even though no column has that name.
//!
//! Limits, by design: a field a transform creates without writing `as` (an
//! aggregate's default name, say) is reported as unknown, so write `as`; the
//! plural `fields` of selections and the `groupby` lists are not read; and a
//! `field` that is not a string (a `{"repeat": ...}` reference) is skipped.

use std::collections::{BTreeSet, HashSet};

use serde_json::Value;

use crate::source::child;

/// The fields found in a specification.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fields {
    /// Each drawn field, with the JSON Pointer of its `field` key.
    pub used: Vec<(String, String)>,
    /// The names transforms create with `as`.
    pub created: BTreeSet<String>,
}

/// Collects the fields of `spec`, which sits at `pointer` in its document.
pub fn collect(spec: &Value, pointer: &str) -> Fields {
    let mut fields = Fields::default();
    walk(spec, pointer, &mut fields);
    fields
}

fn walk(value: &Value, pointer: &str, fields: &mut Fields) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let here = child(pointer, key);
                match (key.as_str(), item) {
                    ("field", Value::String(name)) => fields.used.push((here, name.clone())),
                    ("as", Value::String(name)) => {
                        fields.created.insert(name.clone());
                    }
                    ("as", Value::Array(names)) => {
                        for name in names.iter().filter_map(Value::as_str) {
                            fields.created.insert(name.to_string());
                        }
                    }
                    _ => walk(item, &here, fields),
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &child(pointer, &index.to_string()), fields);
            }
        }
        _ => {}
    }
}

/// Whether a drawn `field` is a column of the query or created by a
/// transform. A field with a path (`a.b`, `a[0]`) reaches into a column, so
/// its first segment is enough; `\.` writes a literal dot.
pub fn is_known(field: &str, columns: &HashSet<&str>, created: &BTreeSet<String>) -> bool {
    let (root, whole) = split_path(field);
    let known = |name: &str| columns.contains(name) || created.contains(name);
    known(&whole) || known(&root)
}

/// The first segment of a field path, and the whole field with `\.` unescaped.
fn split_path(field: &str) -> (String, String) {
    let mut root = String::new();
    let mut whole = String::new();
    let mut in_root = true;
    let mut chars = field.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('.' | '[' | ']' | '\\')) => {
                if let Some(escaped) = chars.next() {
                    whole.push(escaped);
                    if in_root {
                        root.push(escaped);
                    }
                }
            }
            '.' | '[' => {
                in_root = false;
                whole.push(c);
            }
            _ => {
                whole.push(c);
                if in_root {
                    root.push(c);
                }
            }
        }
    }
    (root, whole)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finds_fields_anywhere_and_their_pointers() {
        let spec = json!({
            "mark": "bar",
            "encoding": {"x": {"field": "day"}, "y": {"field": "events", "aggregate": "sum"}},
            "layer": [{"encoding": {"color": {"field": "region"}}}],
            "transform": [{"filter": {"field": "events", "gt": 0}}]
        });
        let fields = collect(&spec, "/charts/0/vega_lite");
        let mut names: Vec<&str> = fields.used.iter().map(|(_, f)| f.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["day", "events", "events", "region"]);
        assert!(
            fields
                .used
                .iter()
                .any(|(p, f)| p == "/charts/0/vega_lite/encoding/x/field" && f == "day")
        );
        assert!(fields.used.iter().any(|(p, f)| p
            == "/charts/0/vega_lite/layer/0/encoding/color/field"
            && f == "region"));
    }

    #[test]
    fn as_creates_fields() {
        let spec = json!({"transform": [
            {"calculate": "datum.a * 2", "as": "double"},
            {"fold": ["a", "b"], "as": ["key", "value"]},
            {"aggregate": [{"op": "sum", "field": "a", "as": "total"}]}
        ]});
        let fields = collect(&spec, "");
        assert_eq!(
            fields
                .created
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["double", "key", "total", "value"]
        );
        assert_eq!(fields.used.len(), 1);
    }

    #[test]
    fn non_string_fields_are_skipped() {
        let spec = json!({"encoding": {"x": {"field": {"repeat": "row"}}}, "fields": ["a"]});
        assert!(collect(&spec, "").used.is_empty());
    }

    #[test]
    fn known_fields() {
        let columns: HashSet<&str> = ["day", "events", "a.b", "meta"].into_iter().collect();
        let created: BTreeSet<String> = ["total".to_string()].into();
        assert!(is_known("day", &columns, &created));
        assert!(is_known("total", &columns, &created));
        assert!(is_known("meta.region", &columns, &created));
        assert!(is_known("meta[0]", &columns, &created));
        assert!(is_known(r"a\.b", &columns, &created));
        assert!(is_known("a.b", &columns, &created));
        assert!(!is_known("region", &columns, &created));
        assert!(!is_known("Day", &columns, &created));
        assert!(!is_known("a.c", &columns, &created));
    }
}
