//! What `validate` can know about an ECharts option without running it: which
//! data columns it names.
//!
//! An ECharts option has no published schema to check it against, so this reads
//! only the places where the option names a column of the rows the page injects
//! (`dataset.source`, an array of row objects):
//!
//! - `encode`: each value is a column name or a list of them (`x`, `y`,
//!   `tooltip`, `itemName`, `lng`, `lat`, ...). A number is a position, not a
//!   name, and is skipped.
//! - `dimensions`: a list of column names, or of objects with a `name`.
//! - `dimension` inside `visualMap`: the column the visual mapping reads.
//!
//! What it cannot see: a column written inside a tooltip's `formatter`
//! template (`{@name}`), a column reached by position, `series.name`, and
//! anything the page's own code adds. A misspelt name there draws nothing or
//! the wrong thing, and `validate` stays silent.

use serde_json::Value;

use crate::source::child;

/// The columns an option names, each with the JSON Pointer of where.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fields {
    pub used: Vec<(String, String)>,
}

/// Collects the column names of `spec`, which sits at `pointer` in its
/// document.
pub fn collect(spec: &Value, pointer: &str) -> Fields {
    let mut fields = Fields::default();
    walk(spec, pointer, false, &mut fields);
    fields
}

fn name(value: &Value, pointer: &str, fields: &mut Fields) {
    match value {
        Value::String(column) => fields.used.push((pointer.to_string(), column.clone())),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                name(item, &child(pointer, &index.to_string()), fields);
            }
        }
        _ => {}
    }
}

fn walk(value: &Value, pointer: &str, in_visual_map: bool, fields: &mut Fields) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let here = child(pointer, key);
                match (key.as_str(), item) {
                    ("encode", Value::Object(channels)) => {
                        for (channel, names) in channels {
                            name(names, &child(&here, channel), fields);
                        }
                    }
                    ("dimensions", Value::Array(items)) => {
                        for (index, dimension) in items.iter().enumerate() {
                            let at = child(&here, &index.to_string());
                            match dimension {
                                Value::Object(o) => {
                                    if let Some(n) = o.get("name") {
                                        name(n, &child(&at, "name"), fields);
                                    }
                                }
                                other => name(other, &at, fields),
                            }
                        }
                    }
                    ("dimension", Value::String(_)) if in_visual_map => name(item, &here, fields),
                    ("visualMap", _) => walk(item, &here, true, fields),
                    _ => walk(item, &here, in_visual_map, fields),
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(
                    item,
                    &child(pointer, &index.to_string()),
                    in_visual_map,
                    fields,
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn names(fields: &Fields) -> Vec<&str> {
        let mut names: Vec<&str> = fields.used.iter().map(|(_, f)| f.as_str()).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn encode_dimensions_and_visual_map_name_columns() {
        let spec = json!({
            "series": [
                {"type": "bar", "encode": {"x": "day", "y": ["a", "b"], "tooltip": 2, "itemName": "region"}},
                {"type": "scatter", "dimensions": ["lng", {"name": "lat"}, {"type": "int"}, 3]}
            ],
            "visualMap": [{"dimension": "magnitude", "min": 0}],
            "xAxis": {"dimension": "not_a_column"}
        });
        let fields = collect(&spec, "/charts/0/echarts");
        assert_eq!(
            names(&fields),
            ["a", "b", "day", "lat", "lng", "magnitude", "region"]
        );
        assert!(
            fields
                .used
                .iter()
                .any(|(p, f)| p == "/charts/0/echarts/series/0/encode/y/1" && f == "b")
        );
        assert!(
            fields
                .used
                .iter()
                .any(|(p, f)| p == "/charts/0/echarts/series/1/dimensions/1/name" && f == "lat")
        );
    }

    #[test]
    fn what_it_cannot_see_is_not_reported() {
        let spec = json!({"tooltip": {"formatter": "{@missing}"}, "series": [{"name": "nope"}]});
        assert!(collect(&spec, "").used.is_empty());
    }
}
