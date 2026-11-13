//! What `validate` and `serve` say when a board is wrong: the file, the line
//! where it can be known, a rule id, and what to change.

use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: Option<usize>,
    pub rule: &'static str,
    pub message: String,
    pub fix: String,
}

impl Diagnostic {
    pub fn new(
        file: &str,
        line: Option<usize>,
        rule: &'static str,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Diagnostic {
            file: file.to_string(),
            line,
            rule,
            message: message.into(),
            fix: fix.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}", self.file)?,
            None => write!(f, "{}", self.file)?,
        }
        write!(
            f,
            ": error[{}]: {}\n    fix: {}",
            self.rule, self.message, self.fix
        )
    }
}

#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    database_checked: bool,
    diagnostics: &'a [Diagnostic],
}

/// The JSON form of a validation run.
pub fn to_json(
    diagnostics: &[Diagnostic],
    database_checked: bool,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&Report {
        ok: diagnostics.is_empty(),
        database_checked,
        diagnostics,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn text_and_json_carry_the_same_facts() {
        let d = Diagnostic::new(
            "boards/a.yml",
            Some(12),
            "some-rule",
            "what is wrong",
            "what to do",
        );
        assert_eq!(
            d.to_string(),
            "boards/a.yml:12: error[some-rule]: what is wrong\n    fix: what to do"
        );
        let json: serde_json::Value = serde_json::from_str(&to_json(&[d], true).unwrap()).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["database_checked"], true);
        assert_eq!(json["diagnostics"][0]["line"], 12);
        assert_eq!(json["diagnostics"][0]["rule"], "some-rule");
        assert_eq!(json["diagnostics"][0]["fix"], "what to do");
    }

    #[test]
    fn a_missing_line_is_null_in_json_and_absent_in_text() {
        let d = Diagnostic::new("dir", None, "r", "m", "f");
        assert!(d.to_string().starts_with("dir: error[r]"));
        let json: serde_json::Value = serde_json::from_str(&to_json(&[d], false).unwrap()).unwrap();
        assert!(json["diagnostics"][0]["line"].is_null());
        let empty: serde_json::Value = serde_json::from_str(&to_json(&[], false).unwrap()).unwrap();
        assert_eq!(empty["ok"], true);
    }
}
