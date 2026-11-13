//! Rewrites a board's `$name` parameters into positional ones.
//!
//! This is a small lexer, not a text replacement: a `$name` inside a string,
//! an identifier, a comment or a dollar-quoted string is text, not a
//! parameter, and replacing it there would change what the query means.
//!
//! What it knows of Postgres' lexical rules: single-quoted strings (`''` for a
//! quote, and backslash escapes after `E`), double-quoted identifiers, `--`
//! line comments, `/* */` block comments that nest, and `$tag$ ... $tag$`
//! strings. It assumes `standard_conforming_strings = on`, the default since
//! Postgres 9.1.

use std::collections::{BTreeMap, HashMap};

use crate::param::ParamType;

/// The SQL with every `$name` replaced, and the names in positional order:
/// `order[0]` is `$1`.
#[derive(Debug, PartialEq, Eq)]
pub struct Rewritten {
    pub sql: String,
    pub order: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Problem {
    /// `$name` where `name` is not declared in `params`.
    Undeclared(String),
    /// `$1`, written by the author.
    Positional(String),
    /// A string, identifier, comment or dollar quote that never closes.
    Unterminated(&'static str),
    /// A `;` followed by more SQL: one query is one statement.
    ExtraStatement,
    /// Nothing but whitespace and comments.
    Empty,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SqlError {
    pub problem: Problem,
    /// The line within the query text, starting at 1.
    pub line: usize,
}

/// The most problems of the kinds that can repeat (an undeclared name, a
/// positional parameter) that are kept for one query. A person, or an agent,
/// fixes the first ones and runs again; 50 is more than fits on a screen, and
/// the cap keeps the memory and the output bounded however large the file is.
/// What is not kept is counted, so the cut is never silent.
pub const MAX_ERRORS: usize = 50;

/// Why a query was refused: the problems found, and how many more of the
/// repeating kinds were found and not kept.
#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub errors: Vec<SqlError>,
    pub omitted: usize,
}

/// Where each line of a text starts, found once, so that the line of an offset
/// is a binary search instead of a scan from the start of the text.
struct Lines {
    newlines: Vec<usize>,
}

impl Lines {
    fn new(text: &str) -> Lines {
        Lines {
            newlines: text
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i)
                .collect(),
        }
    }

    /// The line, from 1, that holds byte `offset`.
    fn line_at(&self, offset: usize) -> usize {
        self.newlines.partition_point(|newline| *newline < offset) + 1
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Rewrites `sql`. A single trailing `;` is dropped.
pub fn rewrite(sql: &str, declared: &BTreeMap<String, ParamType>) -> Result<Rewritten, Failure> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(sql.len() + 16);
    let mut order: Vec<String> = Vec::new();
    let mut positions: HashMap<&str, usize> = HashMap::new(); // name -> position
    let mut errors: Vec<SqlError> = Vec::new();
    let mut omitted = 0;
    let mut flushed = 0; // sql[..flushed] is already in `out`
    let mut i = 0;
    let mut significant = false;
    let mut semicolon: Option<usize> = None; // offset of a `;` seen earlier
    let mut semicolon_reported = false;
    let lines = Lines::new(sql);
    let line_at = |offset: usize| lines.line_at(offset);

    // Something that is not whitespace or a comment starts here: a `;` seen
    // earlier was not the last thing in the query.
    macro_rules! token {
        () => {{
            significant = true;
            if let Some(semi) = semicolon {
                if !semicolon_reported {
                    semicolon_reported = true;
                    errors.push(SqlError {
                        problem: Problem::ExtraStatement,
                        line: line_at(semi),
                    });
                }
            }
        }};
    }
    // A problem that can repeat as often as the query is long.
    macro_rules! repeating {
        ($problem:expr, $at:expr) => {{
            if errors.len() < MAX_ERRORS {
                errors.push(SqlError {
                    problem: $problem,
                    line: line_at($at),
                });
            } else {
                omitted += 1;
            }
        }};
    }
    macro_rules! unterminated {
        ($what:expr, $at:expr) => {{
            errors.push(SqlError {
                problem: Problem::Unterminated($what),
                line: line_at($at),
            });
            break;
        }};
    }

    while i < n {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0c => i += 1,
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let start = i;
                let mut depth = 1;
                i += 2;
                while i < n && depth > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if depth > 0 {
                    unterminated!("block comment", start);
                }
            }
            b'\'' => {
                token!();
                let start = i;
                // E'...' strings treat a backslash as an escape.
                let escapes = i >= 1
                    && matches!(b[i - 1], b'e' | b'E')
                    && (i < 2 || !is_ident_byte(b[i - 2]) && b[i - 2] != b'$');
                i += 1;
                loop {
                    match b.get(i) {
                        None => unterminated!("string", start),
                        Some(b'\\') if escapes => i += 2,
                        Some(b'\'') if b.get(i + 1) == Some(&b'\'') => i += 2,
                        Some(b'\'') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b'"' => {
                token!();
                let start = i;
                i += 1;
                loop {
                    match b.get(i) {
                        None => unterminated!("quoted identifier", start),
                        Some(b'"') if b.get(i + 1) == Some(&b'"') => i += 2,
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b';' => {
                // Drop it; whether it was the last thing is known later.
                out.push_str(&sql[flushed..i]);
                flushed = i + 1;
                if semicolon.is_none() {
                    semicolon = Some(i);
                } else {
                    token!();
                }
                i += 1;
            }
            b'$' => {
                // A `$` inside a word (`a$b`) is part of the identifier.
                if i > 0 && (is_ident_byte(b[i - 1]) || b[i - 1] == b'$') {
                    token!();
                    i += 1;
                    continue;
                }
                token!();
                let start = i;
                let mut j = i + 1;
                while j < n && is_ident_byte(b[j]) {
                    j += 1;
                }
                let tag = &sql[start + 1..j];
                let starts_with_digit = tag.as_bytes().first().is_some_and(u8::is_ascii_digit);
                if !starts_with_digit && b.get(j) == Some(&b'$') {
                    // A dollar-quoted string: `$tag$ ... $tag$`.
                    let delimiter = &sql[start..=j];
                    match sql[j + 1..].find(delimiter) {
                        Some(offset) => i = j + 1 + offset + delimiter.len(),
                        None => unterminated!("dollar-quoted string", start),
                    }
                } else if tag.is_empty() {
                    i += 1; // a lone `$`: Postgres will report it
                } else if starts_with_digit {
                    repeating!(Problem::Positional(tag.to_string()), start);
                    i = j;
                } else if declared.contains_key(tag) {
                    let position = *positions.entry(tag).or_insert_with(|| {
                        order.push(tag.to_string());
                        order.len()
                    });
                    // `declared.contains_key` was true, so the type is there.
                    if let Some(ty) = declared.get(tag) {
                        out.push_str(&sql[flushed..start]);
                        out.push_str(&ty.cast(position));
                        flushed = j;
                    }
                    i = j;
                } else {
                    repeating!(Problem::Undeclared(tag.to_string()), start);
                    i = j;
                }
            }
            _ => {
                token!();
                i += 1;
            }
        }
    }

    if errors.is_empty() && !significant {
        errors.push(SqlError {
            problem: Problem::Empty,
            line: 1,
        });
    }
    if !errors.is_empty() {
        return Err(Failure { errors, omitted });
    }
    out.push_str(&sql[flushed..]);
    Ok(Rewritten { sql: out, order })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn declared(names: &[(&str, ParamType)]) -> BTreeMap<String, ParamType> {
        names.iter().map(|(n, t)| (n.to_string(), *t)).collect()
    }

    fn ok(sql: &str, names: &[(&str, ParamType)]) -> Rewritten {
        rewrite(sql, &declared(names)).unwrap()
    }

    fn problems(sql: &str, names: &[(&str, ParamType)]) -> Vec<(Problem, usize)> {
        rewrite(sql, &declared(names))
            .unwrap_err()
            .errors
            .into_iter()
            .map(|e| (e.problem, e.line))
            .collect()
    }

    #[test]
    fn replaces_a_parameter_with_a_cast_of_its_declared_type() {
        let r = ok(
            "select * from t where d >= $from and r = any($regions)",
            &[("from", ParamType::Date), ("regions", ParamType::ListText)],
        );
        assert_eq!(
            r.sql,
            "select * from t where d >= (($1::text)::date) and r = any(($2::text[]))"
        );
        assert_eq!(r.order, ["from", "regions"]);
    }

    #[test]
    fn a_repeated_parameter_keeps_its_position() {
        let r = ok(
            "select $a, $b, $a",
            &[("a", ParamType::Integer), ("b", ParamType::Text)],
        );
        assert_eq!(
            r.sql,
            "select (($1::text)::bigint), ($2::text), (($1::text)::bigint)"
        );
        assert_eq!(r.order, ["a", "b"]);
    }

    #[test]
    fn positions_follow_first_use_not_declaration() {
        let r = ok(
            "select $b, $a",
            &[("a", ParamType::Text), ("b", ParamType::Text)],
        );
        assert_eq!(r.order, ["b", "a"]);
        assert_eq!(r.sql, "select ($1::text), ($2::text)");
    }

    #[test]
    fn single_quoted_strings_are_left_alone() {
        let r = ok("select '$a and $1 and $$' || $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select '$a and $1 and $$' || ($1::text)");
        // A doubled quote does not end the string.
        let r = ok("select 'it''s $a', $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select 'it''s $a', ($1::text)");
    }

    #[test]
    fn escape_strings_honour_backslash_quotes() {
        // In E'..', \' is an escaped quote, so the string runs to the last '.
        let r = ok(r"select E'a\' $zz', $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, r"select E'a\' $zz', ($1::text)");
        // In a plain string a backslash is a character, so the quote ends it.
        let r = ok(r"select 'a\', $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, r"select 'a\', ($1::text)");
        // `e` at the end of a word is not an escape prefix.
        let r = ok(r"select name'x\', $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, r"select name'x\', ($1::text)");
    }

    #[test]
    fn double_quoted_identifiers_are_left_alone() {
        let r = ok(
            r#"select "col $a" , "q""$a", $a"#,
            &[("a", ParamType::Text)],
        );
        assert_eq!(r.sql, r#"select "col $a" , "q""$a", ($1::text)"#);
    }

    #[test]
    fn comments_are_left_alone() {
        let r = ok(
            "select 1 -- $nope and 'quote\n, $a /* $nope2 */ /* a /* nested $n3 */ still */ ",
            &[("a", ParamType::Text)],
        );
        assert_eq!(
            r.sql,
            "select 1 -- $nope and 'quote\n, ($1::text) /* $nope2 */ /* a /* nested $n3 */ still */ "
        );
    }

    #[test]
    fn dollar_quoted_strings_are_left_alone() {
        let r = ok("select $$ $a 'x $$, $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select $$ $a 'x $$, ($1::text)");
        let r = ok("select $body$ $a $$ $body$, $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select $body$ $a $$ $body$, ($1::text)");
        // A different tag does not close it.
        let r = ok("select $x$ $a $y$ $a $x$", &[]);
        assert_eq!(r.sql, "select $x$ $a $y$ $a $x$");
    }

    #[test]
    fn a_dollar_inside_a_word_is_part_of_the_word() {
        let r = ok("select a$b, $a", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select a$b, ($1::text)");
        let r = ok("select a$b$c from t", &[]);
        assert_eq!(r.sql, "select a$b$c from t");
    }

    #[test]
    fn casts_after_a_parameter_apply_to_the_whole_expression() {
        let r = ok(
            "select $d::date, $l[1]",
            &[("d", ParamType::Date), ("l", ParamType::ListInteger)],
        );
        assert_eq!(
            r.sql,
            "select (($1::text)::date)::date, (($2::text[])::bigint[])[1]"
        );
    }

    #[test]
    fn colons_and_other_operators_pass_through() {
        let r = ok("select x::date, y::text[], a ? b, c @> d from t", &[]);
        assert_eq!(r.sql, "select x::date, y::text[], a ? b, c @> d from t");
    }

    #[test]
    fn undeclared_names_are_reported_with_their_line() {
        let e = problems("select 1\nwhere a = $missing\nand b = $other", &[]);
        assert_eq!(
            e,
            [
                (Problem::Undeclared("missing".to_string()), 2),
                (Problem::Undeclared("other".to_string()), 3)
            ]
        );
    }

    #[test]
    fn positional_parameters_are_refused() {
        let e = problems("select $1, $2", &[]);
        assert_eq!(
            e,
            [
                (Problem::Positional("1".to_string()), 1),
                (Problem::Positional("2".to_string()), 1)
            ]
        );
        // Inside a string it is only text.
        assert!(rewrite("select '$1'", &BTreeMap::new()).is_ok());
    }

    #[test]
    fn unterminated_constructs_are_reported() {
        assert_eq!(
            problems("select 'abc", &[]),
            [(Problem::Unterminated("string"), 1)]
        );
        assert_eq!(
            problems("select E'abc\\'", &[]),
            [(Problem::Unterminated("string"), 1)]
        );
        assert_eq!(
            problems("select \"abc", &[]),
            [(Problem::Unterminated("quoted identifier"), 1)]
        );
        assert_eq!(
            problems("select 1\n/* abc", &[]),
            [(Problem::Unterminated("block comment"), 2)]
        );
        assert_eq!(
            problems("select $$ abc", &[]),
            [(Problem::Unterminated("dollar-quoted string"), 1)]
        );
        assert_eq!(
            problems("select $t$ abc $$", &[]),
            [(Problem::Unterminated("dollar-quoted string"), 1)]
        );
        // A nested comment that is closed only once is not closed.
        assert_eq!(
            problems("/* /* */", &[]),
            [(Problem::Unterminated("block comment"), 1)]
        );
    }

    #[test]
    fn one_statement_with_an_optional_trailing_semicolon() {
        assert_eq!(ok("select 1;", &[]).sql, "select 1");
        assert_eq!(ok("select 1 ;  -- done\n", &[]).sql, "select 1   -- done\n");
        assert_eq!(ok("select ';'", &[]).sql, "select ';'");
        assert_eq!(
            problems("select 1;\nselect 2", &[]),
            [(Problem::ExtraStatement, 1)]
        );
        assert_eq!(problems("select 1;;", &[]), [(Problem::ExtraStatement, 1)]);
    }

    #[test]
    fn empty_queries_are_refused() {
        assert_eq!(problems("", &[]), [(Problem::Empty, 1)]);
        assert_eq!(
            problems("  -- nothing\n /* here */ ", &[]),
            [(Problem::Empty, 1)]
        );
        assert_eq!(problems(";", &[]), [(Problem::Empty, 1)]);
    }

    #[test]
    fn non_ascii_text_survives_the_rewrite() {
        let r = ok("select 'é', \"ñ\", $a, 'ü'", &[("a", ParamType::Text)]);
        assert_eq!(r.sql, "select 'é', \"ñ\", ($1::text), 'ü'");
    }

    #[test]
    fn a_lone_dollar_is_left_for_postgres_to_report() {
        assert_eq!(ok("select $ 1", &[]).sql, "select $ 1");
    }

    #[test]
    fn lines_are_found_for_every_offset_of_a_large_text() {
        // 200,000 lines. A scan from the start for each offset would read
        // about 10^11 bytes; the index reads the text once. The bound is
        // generous: it only catches that quadratic cost, not a slow machine.
        let text = "ab\n".repeat(200_000);
        let started = std::time::Instant::now();
        let lines = Lines::new(&text);
        for line in 0..200_000 {
            assert_eq!(lines.line_at(line * 3), line + 1);
            assert_eq!(lines.line_at(line * 3 + 1), line + 1);
            assert_eq!(lines.line_at(line * 3 + 2), line + 1); // the newline itself
            if line % 1000 == 0 {
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(20),
                    "line {line}: too slow"
                );
            }
        }
    }

    #[test]
    fn many_errors_are_capped_counted_and_still_on_the_right_lines() {
        // One undeclared name a line, 300,000 lines.
        let sql = "$x\n".repeat(300_000);
        let started = std::time::Instant::now();
        let failure = rewrite(&sql, &BTreeMap::new()).unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        assert_eq!(failure.errors.len(), MAX_ERRORS);
        assert_eq!(failure.omitted, 300_000 - MAX_ERRORS);
        for (index, e) in failure.errors.iter().enumerate() {
            assert_eq!(e.problem, Problem::Undeclared("x".to_string()));
            assert_eq!(e.line, index + 1);
        }
    }

    #[test]
    fn fewer_errors_than_the_cap_are_all_kept() {
        let sql = "$x\n".repeat(MAX_ERRORS);
        let failure = rewrite(&sql, &BTreeMap::new()).unwrap_err();
        assert_eq!((failure.errors.len(), failure.omitted), (MAX_ERRORS, 0));
    }

    #[test]
    fn many_distinct_parameters_keep_their_order() {
        let names: Vec<String> = (0..20_000).map(|i| format!("p{i}")).collect();
        let declared: BTreeMap<String, ParamType> = names
            .iter()
            .map(|n| (n.clone(), ParamType::Integer))
            .collect();
        let sql = names
            .iter()
            .rev()
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let r = rewrite(&sql, &declared).unwrap();
        let reversed: Vec<String> = names.iter().rev().cloned().collect();
        assert_eq!(r.order, reversed);
    }
}
