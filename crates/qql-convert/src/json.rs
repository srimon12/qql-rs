//! Strict JSON access helpers.
//!
//! Mirrors the `qql-runtime::rest_response` decode style: an expected field
//! must exist and have the exact type, otherwise decoding fails with a typed
//! [`ConvertError::InvalidField`] naming the path. Request objects reject
//! unknown keys via [`reject_unknown`]; payload-like maps keep arbitrary
//! keys. Known keys with a wrong shape are never coerced.

use serde_json::{Map, Value};

use crate::ConvertError;
use qql_core::ast;

/// JSON object map shorthand.
pub(crate) type Obj = Map<String, Value>;

/// Build a child field path (`parent.key`).
pub(crate) fn child(path: &str, key: &str) -> String {
    let mut out = String::with_capacity(path.len() + key.len() + 1);
    out.push_str(path);
    if !path.is_empty() {
        out.push('.');
    }
    out.push_str(key);
    out
}

/// Build an array element path (`parent[i]`).
pub(crate) fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

/// Fail with a typed invalid-field error.
pub(crate) fn invalid(path: impl Into<String>, detail: impl Into<String>) -> ConvertError {
    ConvertError::invalid(path, detail)
}

/// Reject any object key that is not in `known`.
///
/// Extra OpenAPI properties that QQL cannot represent must fail closed
/// rather than being dropped. Callers pass the fields they decode (and the
/// fields they reject with a more specific message, which they check first).
pub(crate) fn reject_unknown(obj: &Obj, path: &str, known: &[&str]) -> Result<(), ConvertError> {
    for key in obj.keys() {
        if !known.contains(&key.as_str()) {
            return Err(invalid(child(path, key), format!("unknown field '{key}'")));
        }
    }
    Ok(())
}

/// Required string member, returned owned.
pub(crate) fn required_str(obj: &Obj, key: &str, path: &str) -> Result<String, ConvertError> {
    Ok(string_at(required(obj, key, path)?, &child(path, key))?.to_string())
}

/// Access a JSON object, failing closed on any other JSON type.
pub(crate) fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Obj, ConvertError> {
    value.as_object().ok_or_else(|| {
        invalid(
            path,
            format!("expected an object, got {}", type_name(value)),
        )
    })
}

/// Access a JSON array, failing closed on any other JSON type.
pub(crate) fn array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, ConvertError> {
    value
        .as_array()
        .ok_or_else(|| invalid(path, format!("expected an array, got {}", type_name(value))))
}

/// Required object member.
pub(crate) fn required<'a>(obj: &'a Obj, key: &str, path: &str) -> Result<&'a Value, ConvertError> {
    obj.get(key)
        .ok_or_else(|| invalid(child(path, key), "missing required field"))
}

/// Required string member.
pub(crate) fn string_at<'a>(value: &'a Value, path: &str) -> Result<&'a str, ConvertError> {
    value
        .as_str()
        .ok_or_else(|| invalid(path, format!("expected a string, got {}", type_name(value))))
}

/// Required unsigned-integer member.
pub(crate) fn u64_at(value: &Value, path: &str) -> Result<u64, ConvertError> {
    value.as_u64().ok_or_else(|| {
        invalid(
            path,
            format!("expected an unsigned integer, got {}", type_name(value)),
        )
    })
}

/// Required finite-number member; integers are accepted and widened.
pub(crate) fn f64_at(value: &Value, path: &str) -> Result<f64, ConvertError> {
    value.as_f64().filter(|f| f.is_finite()).ok_or_else(|| {
        invalid(
            path,
            format!("expected a finite number, got {}", type_name(value)),
        )
    })
}

/// Required boolean member.
pub(crate) fn bool_at(value: &Value, path: &str) -> Result<bool, ConvertError> {
    value.as_bool().ok_or_else(|| {
        invalid(
            path,
            format!("expected a boolean, got {}", type_name(value)),
        )
    })
}

/// Optional `key` string member.
pub(crate) fn opt_string(obj: &Obj, key: &str, path: &str) -> Result<Option<String>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(string_at(value, &child(path, key))?.to_string())),
    }
}

/// Optional `key` unsigned-integer member.
pub(crate) fn opt_u64(obj: &Obj, key: &str, path: &str) -> Result<Option<u64>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(u64_at(value, &child(path, key))?)),
    }
}

/// Optional `key` finite-number member.
pub(crate) fn opt_f64(obj: &Obj, key: &str, path: &str) -> Result<Option<f64>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(f64_at(value, &child(path, key))?)),
    }
}

/// Optional `key` boolean member.
pub(crate) fn opt_bool(obj: &Obj, key: &str, path: &str) -> Result<Option<bool>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(bool_at(value, &child(path, key))?)),
    }
}

/// Optional `key` object member.
pub(crate) fn opt_object<'a>(
    obj: &'a Obj,
    key: &str,
    path: &str,
) -> Result<Option<&'a Obj>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(object(value, &child(path, key))?)),
    }
}

/// Human-readable JSON type for diagnostics.
pub(crate) fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Convert a JSON value into a QQL AST literal.
///
/// Integers that fit `i64` stay `Int`; integers above `i64::MAX` become
/// `UInt`; other numbers become floats.
pub(crate) fn json_to_ast_value(value: &Value, path: &str) -> Result<ast::Value, ConvertError> {
    Ok(match value {
        Value::Null => ast::Value::Null,
        Value::Bool(b) => ast::Value::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => ast::Value::Int(i),
            None => match n.as_u64() {
                Some(u) => ast::Value::UInt(u),
                None => ast::Value::Float(f64_at(value, path)?),
            },
        },
        Value::String(s) => ast::Value::Str(s.clone()),
        Value::Array(items) => ast::Value::List(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| json_to_ast_value(item, &index(path, i)))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(entries) => ast::Value::Dict(
            entries
                .iter()
                .map(|(key, item)| Ok((key.clone(), json_to_ast_value(item, &child(path, key))?)))
                .collect::<Result<Vec<_>, ConvertError>>()?,
        ),
    })
}
