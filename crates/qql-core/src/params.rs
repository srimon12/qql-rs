//! Parameter binding and prepared query substitution.
//!
//! Provides type-safe substitution of named (`:name`) and positional (`?`)
//! parameter placeholders in QQL query text.
//!
//! ### Placeholders & Syntax Rules
//!
//! - **Named placeholders**: `:name` (e.g. `:category`, `:limit`).
//! - **Positional placeholders**: `?` (sequential 1-to-1 mapping with parameters list).
//!
//! In QQL, `$` is a first-class identifier character (e.g. `$category`, `$1`), so
//! parameter placeholders exclusively use `:name` and `?`. This guarantees that
//! `$`-prefixed identifiers in queries are never accidentally or silently rewritten.
//!
//! Furthermore, a colon `:` is only recognized as a parameter placeholder when it occurs
//! at a valid token boundary (preceded by whitespace, punctuation, or start of query).
//! Colons in compact dictionary syntax (e.g. `{a:b}`, `{'a':b}`) are not placeholders
//! and are preserved without modification. Note that unconventional spacing with whitespace
//! before the colon (`{a :b}`) makes `:b` lexically indistinguishable from a placeholder.
//!
//! Literals and dictionary keys are safely formatted and escaped to prevent
//! query injection breakouts. String literals (`'...'`, `r'...'`, `"""..."""`,
//! and `` `...` ``) and comments (`-- ...`) in the source query are preserved
//! verbatim and never substituted.

use crate::ast::Value;
use crate::error::QqlError;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Check if a string is a simple identifier (starts with ascii alphabetic or `_`,
/// followed by ascii alphanumeric or `_`).
fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Returns true if the `:` at position `i` is at a valid token boundary to begin a parameter placeholder.
///
/// If preceded immediately by an identifier character (`[a-zA-Z0-9_$]`), quote (`'`, `"`, `` ` ``),
/// or closing delimiter (`}`, `]`), the colon is part of dictionary syntax (e.g. `{a:b}`, `{'a':b}`)
/// rather than a placeholder.
fn is_placeholder_start(chars: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = chars[i - 1];
    !(prev.is_ascii_alphanumeric()
        || prev == '_'
        || prev == '$'
        || prev == '\''
        || prev == '"'
        || prev == '`'
        || prev == '}'
        || prev == ']')
}

/// Escape a string literal for QQL single-quoted representation.
fn escape_str_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Convert an AST `Value` into its canonical, safely escaped QQL literal string.
///
/// Returns an error if a float value is non-finite (`NaN` or `infinity`).
pub fn value_to_literal(value: &Value) -> Result<String, QqlError> {
    match value {
        Value::Str(s) => Ok(escape_str_literal(s)),
        Value::Int(n) => Ok(n.to_string()),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(QqlError::validation(
                    "QQL-BIND-INVALID-FLOAT",
                    format!("cannot bind non-finite float value '{}'", f),
                    None,
                ));
            }
            if *f != 0.0 && (f.abs() >= 1e16 || f.abs() < 1e-4) {
                Ok(format!("{:e}", f))
            } else if f.fract() == 0.0 && f.abs() < 1e15 {
                Ok(format!("{:.1}", f))
            } else {
                let s = f.to_string();
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    Ok(format!("{}.0", s))
                } else {
                    Ok(s)
                }
            }
        }
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Null => Ok("null".to_string()),
        Value::List(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&value_to_literal(item)?);
            }
            out.push(']');
            Ok(out)
        }
        Value::Dict(entries) => {
            let mut out = String::from("{");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if is_simple_ident(k) {
                    out.push_str(k);
                } else {
                    out.push_str(&escape_str_literal(k));
                }
                out.push_str(": ");
                out.push_str(&value_to_literal(v)?);
            }
            out.push('}');
            Ok(out)
        }
    }
}

/// Substitute named parameters (`:name`) into `source`.
///
/// `lookup` receives the parameter name without the `:` prefix and returns the bound `Value`.
pub fn bind_named<F>(source: &str, lookup: F) -> Result<String, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let mut out = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // 1. Line comment: skip until newline
        if ch == '-' && i + 1 < len && chars[i + 1] == '-' {
            while i < len && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // 2. Backtick string: preserve verbatim
        if ch == '`' {
            out.push('`');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '`' {
                    break;
                }
            }
            continue;
        }

        // 3. Single-quoted string literal: preserve
        if ch == '\'' {
            out.push('\'');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == '\'' {
                    if i < len && chars[i] == '\'' {
                        out.push('\'');
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            continue;
        }

        // 4. Double-quoted string / identifier: preserve
        if ch == '"' {
            out.push('"');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == '"' {
                    break;
                }
            }
            continue;
        }

        // 5. Positional placeholder in named binder -> detect mixed style
        if ch == '?' {
            return Err(QqlError::validation(
                "QQL-BIND-MIXED-STYLE",
                "query contains positional placeholder '?' — use bind_positional / execute_with_positional_params instead of named parameter binding",
                None,
            ));
        }

        // 6. Named placeholder `:name`
        if ch == ':' && i + 1 < len && is_placeholder_start(&chars, i) {
            let next = chars[i + 1];
            // Disallow :: (type cast or namespace) and require valid identifier start
            if next != ':' && (next.is_ascii_alphabetic() || next == '_') {
                i += 1; // skip ':'
                let mut name = String::new();
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    name.push(chars[i]);
                    i += 1;
                }
                if let Some(val) = lookup(&name) {
                    let lit = value_to_literal(&val)?;
                    out.push_str(&lit);
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-MISSING-PARAM",
                        format!("missing value for named parameter ':{}'", name),
                        None,
                    ));
                }
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    Ok(out)
}

/// Substitute positional parameters (`?`) sequentially into `source`.
pub fn bind_positional(source: &str, params: &[Value]) -> Result<String, QqlError> {
    let mut out = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut param_index = 0;

    while i < len {
        let ch = chars[i];

        // 1. Line comment
        if ch == '-' && i + 1 < len && chars[i + 1] == '-' {
            while i < len && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // 2. Backtick string
        if ch == '`' {
            out.push('`');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '`' {
                    break;
                }
            }
            continue;
        }

        // 3. Single-quoted string literal
        if ch == '\'' {
            out.push('\'');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == '\'' {
                    if i < len && chars[i] == '\'' {
                        out.push('\'');
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            continue;
        }

        // 4. Double-quoted string
        if ch == '"' {
            out.push('"');
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == '"' {
                    break;
                }
            }
            continue;
        }

        // 5. Named placeholder in positional binder -> detect mixed style
        if ch == ':' && i + 1 < len && is_placeholder_start(&chars, i) {
            let next = chars[i + 1];
            if next != ':' && (next.is_ascii_alphabetic() || next == '_') {
                return Err(QqlError::validation(
                    "QQL-BIND-MIXED-STYLE",
                    "query contains named placeholder ':name' — use bind_named / execute_with_params instead of positional parameter binding",
                    None,
                ));
            }
        }

        // 6. Sequential positional placeholder `?`
        if ch == '?' {
            if param_index < params.len() {
                let lit = value_to_literal(&params[param_index])?;
                out.push_str(&lit);
                param_index += 1;
                i += 1;
                continue;
            } else {
                return Err(QqlError::validation(
                    "QQL-BIND-MISSING-PARAM",
                    format!(
                        "positional parameter ? index {} out of range (total provided: {})",
                        param_index + 1,
                        params.len()
                    ),
                    None,
                ));
            }
        }

        out.push(ch);
        i += 1;
    }

    if param_index < params.len() {
        let msg = if param_index == 0 {
            format!(
                "no '?' placeholders found in query, but {} positional parameters were supplied",
                params.len()
            )
        } else {
            format!(
                "too many positional parameters provided: bound {}, but {} were supplied",
                param_index,
                params.len()
            )
        };
        return Err(QqlError::validation("QQL-BIND-UNUSED-PARAMS", msg, None));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn check_value_roundtrip(val: Value) {
        let lit = value_to_literal(&val).expect("value_to_literal should succeed");
        let parsed = Parser::parse_value(&lit).expect("Parser::parse_value should succeed");
        assert_eq!(
            parsed, val,
            "value roundtrip mismatch for literal '{}'",
            lit
        );
    }

    #[test]
    fn test_value_to_literal_escaping() {
        assert_eq!(
            value_to_literal(&Value::Str("O'Connor and \\path".into())).unwrap(),
            "'O''Connor and \\\\path'"
        );
        assert_eq!(value_to_literal(&Value::Int(42)).unwrap(), "42");
        assert_eq!(value_to_literal(&Value::Float(3.75)).unwrap(), "3.75");
        assert_eq!(value_to_literal(&Value::Float(1.0)).unwrap(), "1.0");
        assert_eq!(value_to_literal(&Value::Float(1e300)).unwrap(), "1e300");
        assert_eq!(value_to_literal(&Value::Bool(true)).unwrap(), "true");
        assert_eq!(value_to_literal(&Value::Null).unwrap(), "null");
    }

    #[test]
    fn test_value_ast_equality_roundtrips() {
        check_value_roundtrip(Value::Str("plain".into()));
        check_value_roundtrip(Value::Str(
            "escaped 'quote' and \\backslash\nnewline".into(),
        ));
        check_value_roundtrip(Value::Int(12345));
        check_value_roundtrip(Value::Float(45.5));
        check_value_roundtrip(Value::Float(1.25e-5));
        check_value_roundtrip(Value::Bool(true));
        check_value_roundtrip(Value::Bool(false));
        check_value_roundtrip(Value::Null);
        check_value_roundtrip(Value::List(alloc::vec![
            Value::Int(1),
            Value::Str("two".into()),
            Value::Bool(true),
        ]));
        check_value_roundtrip(Value::Dict(alloc::vec![
            ("simple".into(), Value::Int(1)),
            ("".into(), Value::Str("empty key".into())),
            ("$special_ident".into(), Value::Int(2)),
            ("foo.bar".into(), Value::Int(3)),
            ("a: 1, b".into(), Value::Int(4)),
            ("weird 'quoted'".into(), Value::Bool(false)),
        ]));
    }

    #[test]
    fn test_parse_value_rejects_trailing_tokens() {
        assert!(Parser::parse_value("42 trailing").is_err());
        assert!(Parser::parse_value("'string' extra").is_err());
    }

    #[test]
    fn test_dict_key_escaping_prevents_injection() {
        let dict = Value::Dict(alloc::vec![
            ("simple_key".into(), Value::Int(1)),
            ("a: 1, b".into(), Value::Int(5)),
            ("weird 'quote".into(), Value::Str("val".into())),
        ]);
        let lit = value_to_literal(&dict).unwrap();
        assert_eq!(lit, "{simple_key: 1, 'a: 1, b': 5, 'weird ''quote': 'val'}");

        let query = format!("UPSERT INTO test VALUES {{id: 1, payload: {}}};", lit);
        let parsed = Parser::parse(&query);
        assert!(parsed.is_ok(), "parsed error: {:?}", parsed.err());
    }

    #[test]
    fn test_non_finite_floats_rejected() {
        assert!(value_to_literal(&Value::Float(f64::NAN)).is_err());
        assert!(value_to_literal(&Value::Float(f64::INFINITY)).is_err());
        assert!(value_to_literal(&Value::Float(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn test_dollar_identifiers_never_corrupted() {
        let query =
            "QUERY TEXT 'chest pain' FROM docs WHERE $category = 'medical' AND $1 = 5 LIMIT 5;";
        let bound = bind_named(query, |name| match name {
            "category" => Some(Value::Str("ignored".into())),
            _ => None,
        })
        .unwrap();

        assert_eq!(bound, query);
    }

    #[test]
    fn test_bind_preserves_compact_dict_syntax() {
        // In {id: 1, a:b} or {'a':b}, the colon after 'a' or ''a'' is a dictionary separator, NOT a placeholder :b.
        let query = "UPSERT INTO t VALUES {id: 1, a:b, 'c':d, \"e\":f, `g`:h};";
        let bound = bind_named(query, |name| match name {
            "b" | "d" | "f" | "h" => Some(Value::Int(99)),
            _ => None,
        })
        .unwrap();

        assert_eq!(bound, query);

        // Positional binder must also ignore compact dict colons and not flag false-positive mixed style
        let query_pos = "UPSERT INTO t VALUES {id: ?, a:b};";
        let bound_pos = bind_positional(query_pos, &[Value::Int(1)]).unwrap();
        assert_eq!(bound_pos, "UPSERT INTO t VALUES {id: 1, a:b};");
    }

    #[test]
    fn test_bind_named_variables() {
        let query =
            "QUERY TEXT :q FROM docs WHERE category = :cat AND active = :is_active LIMIT :lim;";
        let result = bind_named(query, |name| match name {
            "q" => Some(Value::Str("chest pain".into())),
            "cat" => Some(Value::Str("medical".into())),
            "is_active" => Some(Value::Bool(true)),
            "lim" => Some(Value::Int(10)),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            result,
            "QUERY TEXT 'chest pain' FROM docs WHERE category = 'medical' AND active = true LIMIT 10;"
        );
    }

    #[test]
    fn test_mixed_placeholder_style_errors() {
        let query_with_q = "QUERY TEXT :q FROM docs LIMIT ?;";
        assert!(bind_named(query_with_q, |_| Some(Value::Str("x".into()))).is_err());

        let query_with_name = "QUERY TEXT ? FROM docs WHERE cat = :cat;";
        assert!(bind_positional(query_with_name, &[Value::Str("x".into())]).is_err());
    }

    #[test]
    fn test_bind_preserves_literals_comments_and_backticks() {
        let query = "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = :status;";
        let result = bind_named(query, |name| match name {
            "status" => Some(Value::Str("active".into())),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            result,
            "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = 'active';"
        );
    }

    #[test]
    fn test_bind_positional_variables() {
        let query = "QUERY TEXT ? FROM docs WHERE tenant = ? AND score >= ? LIMIT ?;";
        let params = alloc::vec![
            Value::Str("acme".into()),
            Value::Str("acme_tenant".into()),
            Value::Float(0.85),
            Value::Int(5),
        ];
        let result = bind_positional(query, &params).unwrap();
        assert_eq!(
            result,
            "QUERY TEXT 'acme' FROM docs WHERE tenant = 'acme_tenant' AND score >= 0.85 LIMIT 5;"
        );
    }

    #[test]
    fn test_bind_positional_count_mismatch() {
        let query = "QUERY TEXT ? FROM docs LIMIT ?;";
        let too_few = alloc::vec![Value::Str("q".into())];
        assert!(bind_positional(query, &too_few).is_err());

        let too_many = alloc::vec![Value::Str("q".into()), Value::Int(5), Value::Int(10)];
        assert!(bind_positional(query, &too_many).is_err());

        let query_no_placeholders = "QUERY TEXT 'test' FROM docs LIMIT 5;";
        let err = bind_positional(query_no_placeholders, &[Value::Int(1)]).unwrap_err();
        assert!(err
            .to_string()
            .contains("no '?' placeholders found in query"));
    }
}
