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
            // with no fractional part inside i64 range bind as Int.
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Ok(ast::Value::Int(n as i64))
            } else {
                Ok(ast::Value::Float(n))
            }
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
