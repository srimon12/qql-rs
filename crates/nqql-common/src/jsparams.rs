//! JS `Unknown` → typed [`ast::Value`] conversion for the NAPI bindings.
//!
//! This replaces napi's `serde_json` auto-conversion on `params`. Plain
//! values convert identically (booleans, JSON-safe integers, finite doubles,
//! strings, nulls, arrays, objects); two cases differ deliberately:
//!
//! - `Float32Array` / `Float64Array` bind as [`ast::Value::F32Array`] in one
//!   memcpy with no per-element walk. (napi's serde layer mangles these into
//!   `{'0': …, '1': …}` index-keyed objects — silently wrong SQL.)
//! - Integer typed arrays bind as integer lists (covers sparse `indices`).
//!   `Buffer`, `ArrayBuffer`, other dtypes, and out-of-range `BigInt` fail
//!   closed with a clear message instead of silent garbage: wrap binary data
//!   in a `Float32Array`/`Float64Array` view first (zero-copy, no movement).
//!
//! - `BigInt` binds exactly (`Int` when it fits `i64`, else `UInt` up to
//!   `u64::MAX`) so snowflake point IDs and scroll cursors round-trip;
//!   integer-valued `number`s beyond the exact-integer range fail closed
//!   naming `BigInt` instead of binding an already-rounded f64.
//!
//! Nested `undefined` inside arrays becomes `Null` and object keys holding
//! `undefined` are dropped — JSON `stringify` semantics, matching what the
//! serde layer did for the shapes it accepted.

use napi::JsValue as _;
use napi::ValueType;
use napi::bindgen_prelude::{
    Float32Array, Float64Array, FromNapiValue as _, Int32Array, JsObjectValue as _, Object,
    Uint32Array, Unknown,
};
use qql_core::ast;
use qql_core::error::QqlError;

fn invalid_params(message: impl Into<String>) -> QqlError {
    QqlError::validation("QQL-BIND-INVALID-PARAMS", message.into(), None)
}

fn napi_err(e: napi::Error) -> QqlError {
    invalid_params(format!("invalid parameter value: {e}"))
}

/// Convert `BigInt` words to a typed [`ast::Value`] without rounding.
///
/// `Int` when the magnitude fits `i64` (including `i64::MIN`), else `UInt`
/// up to `u64::MAX`; anything larger fails closed (`None`) so snowflake
/// point IDs and scroll cursors round-trip exactly. Pure over the words, so
/// it is unit-testable without a JS runtime.
pub fn bigint_words_to_value(sign_bit: bool, words: &[u64]) -> Option<ast::Value> {
    match (sign_bit, words) {
        (false, []) => Some(ast::Value::Int(0)),
        (false, [w]) => Some(if *w <= i64::MAX as u64 {
            ast::Value::Int(*w as i64)
        } else {
            ast::Value::UInt(*w)
        }),
        // `-0n` is zero.
        (true, []) => Some(ast::Value::Int(0)),
        (true, [w]) => {
            if *w <= i64::MAX as u64 {
                Some(ast::Value::Int(-(*w as i64)))
            } else if *w == i64::MIN.unsigned_abs() {
                Some(ast::Value::Int(i64::MIN))
            } else {
                None
            }
        }
        // Two or more words exceed `u64::MAX` in magnitude.
        _ => None,
    }
}

/// Convert any JS value to a typed [`ast::Value`].
pub fn unknown_to_value(v: Unknown) -> Result<ast::Value, QqlError> {
    // Scalars dispatch on type first so they never pay for typed-array
    // probing (one FFI call per failed `from_unknown` adds up over hundreds
    // of vector elements).
    match v.get_type().map_err(napi_err)? {
        ValueType::Undefined | ValueType::Null => Ok(ast::Value::Null),
        ValueType::Boolean => {
            let b = bool::from_unknown(v).map_err(napi_err)?;
            Ok(ast::Value::Bool(b))
        }
        ValueType::Number => {
            let n = f64::from_unknown(v).map_err(napi_err)?;
            if !n.is_finite() {
                return Err(invalid_params("cannot bind non-finite float value"));
            }
            // Preserve integer-ness exactly like the serde layer did: values
            // with no fractional part inside i64 range bind as Int — but only
            // inside the exact-integer range. A larger integer-valued Number
            // is already rounded by the time it arrives as f64, so it fails
            // closed naming BigInt instead of mistargeting a point.
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                if n.abs() > MAX_SAFE_INTEGER_F64 {
                    return Err(invalid_params(
                        "integer parameter exceeds the exact-integer range (2^53 - 1); pass a BigInt instead",
                    ));
                }
                Ok(ast::Value::Int(n as i64))
            } else {
                Ok(ast::Value::Float(n))
            }
        }
        ValueType::BigInt => {
            // Exact integers of any size (scroll cursors carry snowflake u64
            // point IDs back through params); the words are checked, never
            // rounded — same convention as `unknown_opt_to_shard_key`.
            use napi::bindgen_prelude::BigInt;
            let big = BigInt::from_unknown(v).map_err(napi_err)?;
            bigint_words_to_value(big.sign_bit, &big.words)
                .ok_or_else(|| invalid_params("BigInt parameter does not fit in u64"))
        }
        ValueType::String => {
            let s = String::from_unknown(v).map_err(napi_err)?;
            Ok(ast::Value::Str(s))
        }
        ValueType::Object => unknown_object_to_value(v),
        _ => Err(invalid_params(
            "unsupported value type for parameter binding (expected bool, int, float, str, list, dict, Float32Array, or Float64Array)",
        )),
    }
}

/// Convert an f64 typed array to a packed [`ast::Value::F32Array`].
fn typedarray_f64_to_value(a: &Float64Array) -> Result<ast::Value, QqlError> {
    let mut out = Vec::with_capacity(a.len());
    for x in a.as_ref() {
        if !x.is_finite() {
            return Err(invalid_params(
                "cannot bind non-finite float value in Float64Array",
            ));
        }
        out.push(*x as f32);
    }
    Ok(ast::Value::F32Array(out))
}

fn unknown_object_to_value(v: Unknown) -> Result<ast::Value, QqlError> {
    // Typed arrays first: napi's serde layer would mangle these into
    // index-keyed objects. Each `from_unknown` validates the element type.
    // (`is_buffer` is true for every typed-array view, so it cannot gate.)
    if let Ok(a) = Float64Array::from_unknown(v) {
        return typedarray_f64_to_value(&a);
    }
    if let Ok(a) = Float32Array::from_unknown(v) {
        return Ok(ast::Value::F32Array(a.as_ref().to_vec()));
    }
    if let Ok(a) = Int32Array::from_unknown(v) {
        return Ok(ast::Value::List(
            a.as_ref()
                .iter()
                .map(|&i| ast::Value::Int(i64::from(i)))
                .collect(),
        ));
    }
    if let Ok(a) = Uint32Array::from_unknown(v) {
        return Ok(ast::Value::List(
            a.as_ref()
                .iter()
                .map(|&i| ast::Value::Int(i64::from(i)))
                .collect(),
        ));
    }
    // Buffers next: a Buffer IS a Uint8Array subclass, so only a failed
    // typed-array match plus `is_buffer` identifies one — and its owners get
    // the actionable guidance (a view costs nothing, no data movement).
    if v.is_buffer().map_err(napi_err)? {
        return Err(invalid_params(
            "binary Buffer must be wrapped in a Float32Array or Float64Array view first (e.g. new Float64Array(buf.buffer, buf.byteOffset, buf.length / 8))",
        ));
    }
    if v.is_arraybuffer().map_err(napi_err)? || v.is_dataview().map_err(napi_err)? {
        return Err(invalid_params(
            "binary ArrayBuffer must be wrapped in a Float32Array or Float64Array view first",
        ));
    }
    let obj = Object::from_unknown(v).map_err(napi_err)?;
    if obj.is_array().map_err(napi_err)? {
        let len = obj.get_array_length().map_err(napi_err)? as usize;
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            // Holes and explicit undefined read back as undefined.
            let item: Unknown = obj.get_element(i as u32).map_err(napi_err)?;
            if item.get_type().map_err(napi_err)? == ValueType::Undefined {
                items.push(ast::Value::Null);
            } else {
                items.push(unknown_to_value(item)?);
            }
        }
        return Ok(ast::Value::List(items));
    }
    let mut entries = Vec::new();
    for key in Object::keys(&obj).map_err(napi_err)? {
        // Object keys holding undefined are dropped (JSON semantics).
        let item: Unknown = obj.get_named_property(&key).map_err(napi_err)?;
        if item.get_type().map_err(napi_err)? == ValueType::Undefined {
            continue;
        }
        entries.push((key, unknown_to_value(item)?));
    }
    Ok(ast::Value::Dict(entries))
}

/// `None` for absent (`undefined`/`null`) params, mirroring the old
/// `Option<serde_json::Value>` signatures where `null` meant "no params".
pub fn unknown_opt_to_value(v: Unknown) -> Result<Option<ast::Value>, QqlError> {
    match v.get_type().map_err(napi_err)? {
        ValueType::Undefined | ValueType::Null => Ok(None),
        _ => unknown_to_value(v).map(Some),
    }
}

/// Wrong-typed `shardKey`: mirrors core's `value_to_shard_key` code so hosts
/// and QQL agree on the failure.
fn shard_key_type_mismatch(message: impl Into<String>) -> QqlError {
    QqlError::validation("QQL-BIND-TYPE-MISMATCH", message.into(), None)
}

/// Largest integer a JS `number` holds exactly. Larger shard keys must arrive
/// as `BigInt`; silently rounding them would mistarget the request.
const MAX_SAFE_INTEGER_F64: f64 = 9007199254740991.0;

/// Convert any JS value to a typed shard routing key (`None` clears).
///
/// Strings become keywords (empty clears), integers numeric keys — the same
/// split the reference parser enforces, so host-set tenants route exactly
/// like their QQL spelling. Anything else fails closed: booleans are rejected
/// outright (`true` as shard `1` would be silent mistargeting), non-integers
/// and out-of-range numbers name `BigInt` instead of rounding.
pub fn unknown_opt_to_shard_key(v: Unknown) -> Result<Option<ast::ShardKey>, QqlError> {
    use napi::bindgen_prelude::FromNapiValue as _;
    let bad_type = || {
        shard_key_type_mismatch(
            "shardKey must be a string, an integer, a BigInt, null, or undefined",
        )
    };
    match v.get_type().map_err(napi_err)? {
        ValueType::Undefined | ValueType::Null => Ok(None),
        ValueType::String => {
            let s = String::from_unknown(v).map_err(napi_err)?;
            Ok(if s.is_empty() {
                None
            } else {
                Some(ast::ShardKey::Keyword(s))
            })
        }
        ValueType::Number => {
            let n = f64::from_unknown(v).map_err(napi_err)?;
            if !n.is_finite() || n.fract() != 0.0 || n < 0.0 {
                return Err(shard_key_type_mismatch(
                    "shardKey number must be a non-negative integer",
                ));
            }
            if n > MAX_SAFE_INTEGER_F64 {
                return Err(shard_key_type_mismatch(
                    "shardKey number exceeds the exact-integer range; pass a BigInt instead",
                ));
            }
            Ok(Some(ast::ShardKey::Number(n as u64)))
        }
        ValueType::BigInt => {
            // Exact integers of any size; the words are checked, never assumed.
            use napi::bindgen_prelude::{BigInt, FromNapiValue as _};
            let big = BigInt::from_unknown(v).map_err(napi_err)?;
            match (big.sign_bit, big.words.as_slice()) {
                (false, [] | [0]) => Ok(Some(ast::ShardKey::Number(0))),
                (false, [n]) => Ok(Some(ast::ShardKey::Number(*n))),
                _ => Err(shard_key_type_mismatch(
                    "shardKey BigInt does not fit in u64",
                )),
            }
        }
        _ => Err(bad_type()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigint_words_bind_exactly() {
        // Zero, both spellings (`0n` arrives with or without words).
        assert_eq!(bigint_words_to_value(false, &[]), Some(ast::Value::Int(0)));
        assert_eq!(bigint_words_to_value(false, &[0]), Some(ast::Value::Int(0)));
        assert_eq!(bigint_words_to_value(true, &[]), Some(ast::Value::Int(0)));
        // Small magnitudes keep their sign as `Int`.
        assert_eq!(
            bigint_words_to_value(false, &[42]),
            Some(ast::Value::Int(42))
        );
        assert_eq!(
            bigint_words_to_value(true, &[42]),
            Some(ast::Value::Int(-42))
        );
        // Real geosmart snowflake: fits `i64`, so `Int` — exact either way.
        assert_eq!(
            bigint_words_to_value(false, &[1479834607549681654]),
            Some(ast::Value::Int(1479834607549681654))
        );
        // Above `i64::MAX` becomes `UInt` — exact.
        assert_eq!(
            bigint_words_to_value(false, &[i64::MAX as u64 + 1]),
            Some(ast::Value::UInt(i64::MAX as u64 + 1))
        );
        assert_eq!(
            bigint_words_to_value(false, &[u64::MAX]),
            Some(ast::Value::UInt(u64::MAX))
        );
        // `i64` edges keep `Int`, including `i64::MIN`.
        assert_eq!(
            bigint_words_to_value(false, &[i64::MAX as u64]),
            Some(ast::Value::Int(i64::MAX))
        );
        assert_eq!(
            bigint_words_to_value(true, &[i64::MIN.unsigned_abs()]),
            Some(ast::Value::Int(i64::MIN))
        );
        // Negative beyond `i64::MIN` and multi-word magnitudes fail closed.
        assert_eq!(
            bigint_words_to_value(true, &[i64::MIN.unsigned_abs() + 1]),
            None
        );
        assert_eq!(bigint_words_to_value(false, &[1, 1]), None);
        assert_eq!(bigint_words_to_value(true, &[1, 1]), None);
    }
}
