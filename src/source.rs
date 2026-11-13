//! Reads a board's YAML into a JSON value, and remembers on which line every
//! key and list item started.
//!
//! Validation needs both halves: serde checks the shape of the value, and the
//! line map is how an error found later (a type that does not parse, a `$name`
//! that is not declared) is attached to the line the author has to edit.

use std::collections::HashMap;

use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, ScanError, YamlData};
use saphyr_parser::{Event, Parser};
use serde_json::{Map, Number, Value};

/// How deep a board may nest. A Vega-Lite specification stays far below this;
/// the limit only stops a hostile file from exhausting the stack.
const MAX_DEPTH: usize = 100;

/// A parsed board file: the value, and where each part of it came from.
#[derive(Debug)]
pub struct Document {
    pub value: Value,
    lines: HashMap<String, usize>,
}

/// A file that is not usable YAML for a board.
#[derive(Debug)]
pub struct SourceError {
    pub line: Option<usize>,
    pub rule: &'static str,
    pub message: String,
    pub fix: String,
}

impl Document {
    /// The line where the part at `pointer` (a JSON Pointer, RFC 6901) starts.
    /// A key points at its own line; a part that has no line of its own, such
    /// as a missing field, falls back to its closest parent that has one.
    pub fn line(&self, pointer: &str) -> Option<usize> {
        let mut current = pointer;
        loop {
            if let Some(line) = self.lines.get(current) {
                return Some(*line);
            }
            let (parent, _) = current.rsplit_once('/')?;
            current = parent;
        }
    }
}

/// Appends one key or index to a JSON Pointer.
pub fn child(pointer: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{pointer}/{escaped}")
}

pub fn parse(text: &str) -> Result<Document, SourceError> {
    reject_anchors_and_tags(text)?;
    let documents = MarkedYaml::load_from_str(text).map_err(syntax_error)?;
    let mut documents = documents.into_iter();
    let (Some(root), None) = (documents.next(), documents.next()) else {
        return Err(SourceError {
            line: None,
            rule: "yaml-unsupported",
            message: "the file must hold exactly one YAML document".to_string(),
            fix: "keep one board per file, with no `---` separators".to_string(),
        });
    };
    let mut lines = HashMap::new();
    let value = convert(&root, String::new(), 0, &mut lines)?;
    Ok(Document { value, lines })
}

fn syntax_error(e: ScanError) -> SourceError {
    SourceError {
        line: Some(e.marker().line()),
        rule: "yaml-syntax",
        message: format!("the file is not valid YAML: {}", e.info()),
        fix: "fix the YAML syntax at that line".to_string(),
    }
}

/// Refuses anchors, aliases and tags before the tree is built. The loader
/// expands an alias by copying what it points to, so a small file of nested
/// aliases could be made to fill the memory ("billion laughs"); a board has
/// no use for them.
fn reject_anchors_and_tags(text: &str) -> Result<(), SourceError> {
    for item in Parser::new_from_str(text) {
        let (event, span) = item.map_err(syntax_error)?;
        let what = match &event {
            Event::Alias(_) => Some("YAML aliases are not supported"),
            Event::Scalar(_, _, anchor, tag) if *anchor != 0 || tag.is_some() => {
                Some("YAML anchors and tags are not supported")
            }
            Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag)
                if *anchor != 0 || tag.is_some() =>
            {
                Some("YAML anchors and tags are not supported")
            }
            _ => None,
        };
        if let Some(message) = what {
            return Err(SourceError {
                line: Some(span.start.line()),
                rule: "yaml-unsupported",
                message: message.to_string(),
                fix: "write the repeated value out in full, and remove the tag".to_string(),
            });
        }
    }
    Ok(())
}

fn unsupported(node: &MarkedYaml, message: &str, fix: &str) -> SourceError {
    SourceError {
        line: Some(node.span.start.line()),
        rule: "yaml-unsupported",
        message: message.to_string(),
        fix: fix.to_string(),
    }
}

fn convert(
    node: &MarkedYaml,
    pointer: String,
    depth: usize,
    lines: &mut HashMap<String, usize>,
) -> Result<Value, SourceError> {
    if depth > MAX_DEPTH {
        return Err(unsupported(
            node,
            &format!("the board nests deeper than {MAX_DEPTH} levels"),
            "flatten the structure",
        ));
    }
    lines
        .entry(pointer.clone())
        .or_insert(node.span.start.line());
    match &node.data {
        YamlData::Value(scalar) => scalar_value(node, scalar),
        YamlData::Sequence(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                out.push(convert(
                    item,
                    child(&pointer, &index.to_string()),
                    depth + 1,
                    lines,
                )?);
            }
            Ok(Value::Array(out))
        }
        YamlData::Mapping(entries) => {
            let mut out = Map::new();
            for (key, item) in entries {
                let YamlData::Value(Scalar::String(name)) = &key.data else {
                    return Err(unsupported(
                        key,
                        "a key is not a string",
                        "quote the key so that it is read as text",
                    ));
                };
                let key_pointer = child(&pointer, name);
                // A key points at its own line, which is where an author looks.
                lines.insert(key_pointer.clone(), key.span.start.line());
                out.insert(
                    name.to_string(),
                    convert(item, key_pointer, depth + 1, lines)?,
                );
            }
            Ok(Value::Object(out))
        }
        YamlData::Alias(_) => Err(unsupported(
            node,
            "YAML anchors and aliases are not supported",
            "write the repeated value out in full",
        )),
        YamlData::Tagged(..) => Err(unsupported(
            node,
            "YAML tags are not supported",
            "remove the tag",
        )),
        YamlData::Representation(..) | YamlData::BadValue => Err(unsupported(
            node,
            "this value could not be read",
            "check the YAML at that line",
        )),
    }
}

fn scalar_value(node: &MarkedYaml, scalar: &Scalar) -> Result<Value, SourceError> {
    match scalar {
        Scalar::Null => Ok(Value::Null),
        Scalar::Boolean(b) => Ok(Value::Bool(*b)),
        Scalar::Integer(i) => Ok(Value::Number(Number::from(*i))),
        Scalar::FloatingPoint(f) => Number::from_f64(f.0).map(Value::Number).ok_or_else(|| {
            unsupported(
                node,
                "a number is not finite",
                "write a finite number, or quote the text",
            )
        }),
        Scalar::String(s) => Ok(Value::String(s.to_string())),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lines_point_at_keys_and_items() {
        let doc =
            parse("board: a\nqueries:\n  q:\n    sql: |\n      select 1\ncharts:\n  - title: t\n")
                .unwrap();
        assert_eq!(doc.line("/board"), Some(1));
        assert_eq!(doc.line("/queries/q/sql"), Some(4));
        assert_eq!(doc.line("/charts/0/title"), Some(7));
        // A path that does not exist falls back to its closest parent.
        assert_eq!(doc.line("/queries/q/params"), Some(3));
    }

    #[test]
    fn plain_dates_stay_text_and_yes_is_not_a_boolean() {
        let doc = parse("a: 2020-01-01\nb: yes\nc: 5\nd: 1.5\n").unwrap();
        assert_eq!(doc.value["a"], "2020-01-01");
        assert_eq!(doc.value["b"], "yes");
        assert_eq!(doc.value["c"], 5);
        assert_eq!(doc.value["d"], 1.5);
    }

    #[test]
    fn keys_with_slashes_are_escaped_in_pointers() {
        let doc = parse("a/b:\n  c: 1\n").unwrap();
        assert_eq!(doc.line("/a~1b/c"), Some(2));
    }

    #[test]
    fn duplicate_keys_are_a_syntax_error() {
        let err = parse("a: 1\na: 2\n").unwrap_err();
        assert_eq!(err.rule, "yaml-syntax");
        assert_eq!(err.line, Some(2));
    }

    #[test]
    fn broken_yaml_reports_its_line() {
        let err = parse("a: 1\nb: [1, 2\n").unwrap_err();
        assert_eq!(err.rule, "yaml-syntax");
        assert!(err.line.is_some());
    }

    #[test]
    fn anchors_aliases_tags_and_extra_documents_are_refused() {
        assert_eq!(
            parse("a: &x 1\nb: *x\n").unwrap_err().rule,
            "yaml-unsupported"
        );
        assert_eq!(parse("a: !!str 1\n").unwrap_err().rule, "yaml-unsupported");
        assert_eq!(
            parse("a: 1\n---\nb: 2\n").unwrap_err().rule,
            "yaml-unsupported"
        );
        assert_eq!(parse("").unwrap_err().rule, "yaml-unsupported");
        assert_eq!(parse("1: a\n").unwrap_err().rule, "yaml-unsupported");
    }

    #[test]
    fn deep_nesting_is_refused() {
        let text = format!("a: {}1{}\n", "[".repeat(150), "]".repeat(150));
        assert_eq!(parse(&text).unwrap_err().rule, "yaml-unsupported");
    }
}
