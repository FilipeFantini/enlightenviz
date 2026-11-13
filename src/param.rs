//! Typed query parameters: what a board may declare, and how a value from a
//! request or a default is checked and turned into text for Postgres.
//!
//! Every value is validated here, in Rust, so that a bad value is a precise 400
//! and never a database error. It is then sent to Postgres as text, and the
//! rewritten SQL casts it back (`(($1::text)::date)`), so the type handling
//! lives in this one file.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Lists have no more items than this unless a parameter says `max_items`.
pub const DEFAULT_MAX_ITEMS: usize = 100;
/// The largest `max_items` a board may ask for.
pub const MAX_ITEMS_CEILING: usize = 1000;

/// The type of a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum ParamType {
    /// Any text, without a NUL character. Sent to Postgres as `text`.
    #[serde(rename = "text")]
    Text,
    /// A whole number in the range of a 64-bit signed integer (`bigint`).
    #[serde(rename = "integer")]
    Integer,
    /// A finite number (`double precision`).
    #[serde(rename = "number")]
    Number,
    /// `true` or `false`.
    #[serde(rename = "boolean")]
    Boolean,
    /// A calendar date written `YYYY-MM-DD` (`date`).
    #[serde(rename = "date")]
    Date,
    /// A list of `text`.
    #[serde(rename = "list<text>")]
    ListText,
    /// A list of `integer`.
    #[serde(rename = "list<integer>")]
    ListInteger,
    /// A list of `number`.
    #[serde(rename = "list<number>")]
    ListNumber,
    /// A list of `boolean`.
    #[serde(rename = "list<boolean>")]
    ListBoolean,
    /// A list of `date`.
    #[serde(rename = "list<date>")]
    ListDate,
}

/// The type of one value, or of each item of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scalar {
    Text,
    Integer,
    Number,
    Boolean,
    Date,
}

impl Scalar {
    fn sql_type(self) -> &'static str {
        match self {
            Scalar::Text => "text",
            Scalar::Integer => "bigint",
            Scalar::Number => "double precision",
            Scalar::Boolean => "boolean",
            Scalar::Date => "date",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Scalar::Text => "text without a NUL character",
            Scalar::Integer => "a whole number that fits in 64 bits",
            Scalar::Number => "a finite number",
            Scalar::Boolean => "`true` or `false`",
            Scalar::Date => "a date written YYYY-MM-DD",
        }
    }
}

impl ParamType {
    pub fn scalar(self) -> Scalar {
        match self {
            ParamType::Text | ParamType::ListText => Scalar::Text,
            ParamType::Integer | ParamType::ListInteger => Scalar::Integer,
            ParamType::Number | ParamType::ListNumber => Scalar::Number,
            ParamType::Boolean | ParamType::ListBoolean => Scalar::Boolean,
            ParamType::Date | ParamType::ListDate => Scalar::Date,
        }
    }

    pub fn is_list(self) -> bool {
        matches!(
            self,
            ParamType::ListText
                | ParamType::ListInteger
                | ParamType::ListNumber
                | ParamType::ListBoolean
                | ParamType::ListDate
        )
    }

    /// The SQL that stands for parameter number `position` (1-based): the
    /// value arrives as `text` (or `text[]`) and is cast to the declared type.
    /// The whole expression is parenthesised so that whatever the author
    /// wrote after it (`::int`, `[1]`) applies to the result.
    pub fn cast(self, position: usize) -> String {
        let list = if self.is_list() { "[]" } else { "" };
        match self.scalar() {
            Scalar::Text => format!("(${position}::text{list})"),
            other => format!("((${position}::text{list})::{}{list})", other.sql_type()),
        }
    }
}

/// A checked value, ready to be sent to Postgres as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    One(String),
    Many(Vec<String>),
}

/// A declared parameter, with its default already checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSpec {
    pub ty: ParamType,
    pub default: Option<Bound>,
    pub max_items: usize,
}

/// Why a declaration is wrong, as a rule id and a message.
#[derive(Debug)]
pub struct SpecError {
    pub rule: &'static str,
    pub message: String,
    pub fix: String,
}

impl ParamSpec {
    /// Checks a declaration: `max_items` only on lists and within the
    /// ceiling, and a default that is a valid value of the type.
    pub fn declare(
        ty: ParamType,
        default: Option<&Value>,
        max_items: Option<usize>,
    ) -> Result<ParamSpec, SpecError> {
        if max_items.is_some() && !ty.is_list() {
            return Err(SpecError {
                rule: "param-max-items-invalid",
                message: "`max_items` applies to list parameters only".to_string(),
                fix: "remove `max_items`, or make the type a list".to_string(),
            });
        }
        let max_items = max_items.unwrap_or(DEFAULT_MAX_ITEMS);
        if max_items == 0 || max_items > MAX_ITEMS_CEILING {
            return Err(SpecError {
                rule: "param-max-items-invalid",
                message: format!(
                    "`max_items` is {max_items}, and must be from 1 to {MAX_ITEMS_CEILING}"
                ),
                fix: format!("use a value from 1 to {MAX_ITEMS_CEILING}"),
            });
        }
        let mut spec = ParamSpec {
            ty,
            default: None,
            max_items,
        };
        if let Some(value) = default {
            spec.default = Some(spec.parse_default(value).map_err(|message| SpecError {
                rule: "param-default-invalid",
                message: format!("the default is not a valid `{}`: {message}", type_name(ty)),
                fix: "write a default of the declared type, in YAML as a quoted string for text and dates, a number for integer and number, true or false for boolean, and a list for list types".to_string(),
            })?);
        }
        Ok(spec)
    }

    fn parse_default(&self, value: &Value) -> Result<Bound, String> {
        let scalar = self.ty.scalar();
        if !self.ty.is_list() {
            return default_item(scalar, value).map(Bound::One);
        }
        let Value::Array(items) = value else {
            return Err("expected a list".to_string());
        };
        if items.len() > self.max_items {
            return Err(format!("the list has more than {} items", self.max_items));
        }
        items
            .iter()
            .map(|item| default_item(scalar, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Bound::Many)
    }

    /// Checks the values a request gave for this parameter (every occurrence
    /// of its key in the query string, in order).
    pub fn bind(&self, values: &[String]) -> Result<Bound, String> {
        let scalar = self.ty.scalar();
        if !self.ty.is_list() {
            return match values {
                [one] => parse_scalar(scalar, one).map(Bound::One),
                _ => Err("must be given exactly once".to_string()),
            };
        }
        if values.len() > self.max_items {
            return Err(format!("accepts at most {} values", self.max_items));
        }
        values
            .iter()
            .map(|v| parse_scalar(scalar, v))
            .collect::<Result<Vec<_>, _>>()
            .map(Bound::Many)
    }
}

pub fn type_name(ty: ParamType) -> String {
    let scalar = match ty.scalar() {
        Scalar::Text => "text",
        Scalar::Integer => "integer",
        Scalar::Number => "number",
        Scalar::Boolean => "boolean",
        Scalar::Date => "date",
    };
    if ty.is_list() {
        format!("list<{scalar}>")
    } else {
        scalar.to_string()
    }
}

/// A YAML default is typed: a string for text and dates, a number for the
/// numeric types, a bool for boolean. Anything else is refused, not coerced.
fn default_item(scalar: Scalar, value: &Value) -> Result<String, String> {
    let raw = match (scalar, value) {
        (Scalar::Text | Scalar::Date, Value::String(s)) => s.clone(),
        (Scalar::Integer | Scalar::Number, Value::Number(n)) => n.to_string(),
        (Scalar::Boolean, Value::Bool(b)) => b.to_string(),
        _ => return Err(format!("expected {}", scalar.describe())),
    };
    parse_scalar(scalar, &raw)
}

/// Checks one value against its type and returns its canonical text.
pub fn parse_scalar(scalar: Scalar, raw: &str) -> Result<String, String> {
    let invalid = || format!("must be {}", scalar.describe());
    match scalar {
        Scalar::Text => {
            if raw.contains('\0') {
                return Err(invalid());
            }
            Ok(raw.to_string())
        }
        Scalar::Integer => raw
            .parse::<i64>()
            .map(|n| n.to_string())
            .map_err(|_| invalid()),
        Scalar::Number => match raw.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n.to_string()),
            _ => Err(invalid()),
        },
        Scalar::Boolean => match raw {
            "true" | "false" => Ok(raw.to_string()),
            _ => Err(invalid()),
        },
        Scalar::Date => parse_date(raw)
            .ok_or_else(invalid)
            .map(|()| raw.to_string()),
    }
}

/// `YYYY-MM-DD`, years 0001 to 9999, a day that exists in that month.
fn parse_date(raw: &str) -> Option<()> {
    let bytes = raw.as_bytes();
    let shape_ok = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    if !shape_ok {
        return None;
    }
    // Every byte is ASCII now, so slicing cannot split a character.
    let (year, month, day): (u32, u32, u32) = (
        raw[0..4].parse().ok()?,
        raw[5..7].parse().ok()?,
        raw[8..10].parse().ok()?,
    );
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (year >= 1 && (1..=days).contains(&day)).then_some(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn integers() {
        assert_eq!(parse_scalar(Scalar::Integer, "42").unwrap(), "42");
        assert_eq!(parse_scalar(Scalar::Integer, "+7").unwrap(), "7");
        assert_eq!(parse_scalar(Scalar::Integer, "-0").unwrap(), "0");
        for bad in [
            "",
            "1.5",
            "1e3",
            "abc",
            "9223372036854775808",
            " 1",
            "1 ",
            "0x10",
        ] {
            assert!(parse_scalar(Scalar::Integer, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_scalar(Scalar::Number, "1.5").unwrap(), "1.5");
        assert_eq!(parse_scalar(Scalar::Number, "2").unwrap(), "2");
        assert_eq!(parse_scalar(Scalar::Number, "1e3").unwrap(), "1000");
        for bad in ["", "NaN", "inf", "-inf", "infinity", "1e999", "x", "1,5"] {
            assert!(parse_scalar(Scalar::Number, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn booleans_are_exactly_true_or_false() {
        assert!(parse_scalar(Scalar::Boolean, "true").is_ok());
        assert!(parse_scalar(Scalar::Boolean, "false").is_ok());
        for bad in ["True", "1", "yes", "", "t"] {
            assert!(parse_scalar(Scalar::Boolean, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn dates() {
        for good in ["2024-02-29", "2000-02-29", "0001-01-01", "9999-12-31"] {
            assert!(parse_scalar(Scalar::Date, good).is_ok(), "{good}");
        }
        for bad in [
            "2023-02-29",
            "1900-02-29",
            "2024-13-01",
            "2024-00-10",
            "2024-04-31",
            "0000-01-01",
            "2024-1-1",
            "24-01-01",
            "2024/01/01",
            "20240101",
            "2024-01-01 ",
            "2024-01-0é",
            "２０２４-01-01",
            "",
        ] {
            assert!(parse_scalar(Scalar::Date, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn text_refuses_only_nul() {
        assert_eq!(
            parse_scalar(Scalar::Text, "a'; drop table x; --").unwrap(),
            "a'; drop table x; --"
        );
        assert_eq!(parse_scalar(Scalar::Text, "").unwrap(), "");
        assert!(parse_scalar(Scalar::Text, "a\0b").is_err());
    }

    #[test]
    fn cast_expressions() {
        assert_eq!(ParamType::Text.cast(1), "($1::text)");
        assert_eq!(ParamType::Date.cast(2), "(($2::text)::date)");
        assert_eq!(ParamType::Integer.cast(3), "(($3::text)::bigint)");
        assert_eq!(ParamType::Number.cast(1), "(($1::text)::double precision)");
        assert_eq!(ParamType::Boolean.cast(1), "(($1::text)::boolean)");
        assert_eq!(ParamType::ListText.cast(1), "($1::text[])");
        assert_eq!(ParamType::ListInteger.cast(4), "(($4::text[])::bigint[])");
        assert_eq!(ParamType::ListDate.cast(1), "(($1::text[])::date[])");
    }

    #[test]
    fn defaults_are_typed() {
        let spec = ParamSpec::declare(ParamType::Date, Some(&json!("2020-01-31")), None).unwrap();
        assert_eq!(spec.default, Some(Bound::One("2020-01-31".to_string())));
        let spec = ParamSpec::declare(ParamType::ListInteger, Some(&json!([1, 2])), None).unwrap();
        assert_eq!(
            spec.default,
            Some(Bound::Many(vec!["1".to_string(), "2".to_string()]))
        );
        let spec = ParamSpec::declare(ParamType::ListText, Some(&json!([])), None).unwrap();
        assert_eq!(spec.default, Some(Bound::Many(vec![])));
        for (ty, value) in [
            (ParamType::Date, json!("2020-02-30")),
            (ParamType::Date, json!(20200101)),
            (ParamType::Text, json!(5)),
            (ParamType::Integer, json!("5")),
            (ParamType::Integer, json!(1.5)),
            (ParamType::Boolean, json!("true")),
            (ParamType::ListText, json!("a")),
            (ParamType::Text, json!(["a"])),
            (ParamType::ListInteger, json!([1, "2"])),
        ] {
            let err = ParamSpec::declare(ty, Some(&value), None).unwrap_err();
            assert_eq!(err.rule, "param-default-invalid", "{ty:?} {value}");
        }
    }

    #[test]
    fn max_items_rules() {
        assert_eq!(
            ParamSpec::declare(ParamType::ListText, None, None)
                .unwrap()
                .max_items,
            DEFAULT_MAX_ITEMS
        );
        assert_eq!(
            ParamSpec::declare(ParamType::ListText, None, Some(3))
                .unwrap()
                .max_items,
            3
        );
        for (ty, max) in [
            (ParamType::Text, Some(3)),
            (ParamType::ListText, Some(0)),
            (ParamType::ListText, Some(MAX_ITEMS_CEILING + 1)),
        ] {
            let err = ParamSpec::declare(ty, None, max).unwrap_err();
            assert_eq!(err.rule, "param-max-items-invalid");
        }
        let err =
            ParamSpec::declare(ParamType::ListText, Some(&json!(["a", "b"])), Some(1)).unwrap_err();
        assert_eq!(err.rule, "param-default-invalid");
    }

    #[test]
    fn binding_requests() {
        let scalar = ParamSpec::declare(ParamType::Integer, None, None).unwrap();
        assert_eq!(
            scalar.bind(&["5".to_string()]).unwrap(),
            Bound::One("5".to_string())
        );
        assert!(scalar.bind(&[]).is_err());
        assert!(scalar.bind(&["1".to_string(), "2".to_string()]).is_err());
        assert!(scalar.bind(&["x".to_string()]).is_err());

        let list = ParamSpec::declare(ParamType::ListText, None, Some(2)).unwrap();
        let two = ["a".to_string(), "b".to_string()];
        assert_eq!(list.bind(&two).unwrap(), Bound::Many(two.to_vec()));
        assert!(
            list.bind(&["a".to_string(), "b".to_string(), "c".to_string()])
                .is_err()
        );
        let ints = ParamSpec::declare(ParamType::ListInteger, None, None).unwrap();
        assert!(ints.bind(&["1".to_string(), "x".to_string()]).is_err());
    }

    #[test]
    fn type_names_round_trip_through_serde() {
        for name in [
            "text",
            "integer",
            "number",
            "boolean",
            "date",
            "list<text>",
            "list<date>",
        ] {
            let ty: ParamType = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(type_name(ty), name);
        }
        assert!(serde_json::from_value::<ParamType>(json!("list<list<text>>")).is_err());
    }
}
