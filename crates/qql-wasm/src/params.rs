//! Parameter plumbing: option parsing plus the JSON and typed bind contracts.
//!
//! [`jsvalue_to_value`] is the single JsValue converter for the crate: plain
//! JSON values convert exactly like `serde-wasm-bindgen`, while
//! `Float32Array` / `Float64Array` bind as packed `f32` vectors and integer
//! typed arrays bind as integer lists (sparse `indices`). Anything else
//! fails closed with wrap-first guidance.

use qql_core::ast::{self, Value};
use qql_core::error::QqlError;
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WasmOnError {
    Stop,
    Continue,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn parse_on_error(options: Option<&JsValue>) -> Result<WasmOnError, JsValue> {
    let Some(options) = options else {
        return Ok(WasmOnError::Stop);
    };
    if options.is_null() || options.is_undefined() {
        return Ok(WasmOnError::Stop);
    }
    if !options.is_object() {
        return Err(JsValue::from_str("options must be an object"));
    }
    let value = js_sys::Reflect::get(options, &JsValue::from_str("onError"))?;
    if value.is_undefined() {
        return Ok(WasmOnError::Stop);
    }
    match value.as_string().as_deref() {
        Some("stop") => Ok(WasmOnError::Stop),
        Some("continue") => Ok(WasmOnError::Continue),
        _ => Err(JsValue::from_str(
            "options.onError must be 'stop' or 'continue'",
        )),
    }
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn options_params(options: Option<&JsValue>) -> Result<Option<Value>, JsValue> {
    let Some(options) = options else {
        return Ok(None);
    };
    if options.is_null() || options.is_undefined() || !options.is_object() {
        return Ok(None);
    }
    let value = js_sys::Reflect::get(options, &JsValue::from_str("params"))?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    jsvalue_to_value(&value)
        .map(Some)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn extract_ast_stmt(val: &JsValue) -> Option<ast::Stmt> {
    if let Ok(to_obj_val) = js_sys::Reflect::get(val, &JsValue::from_str("toObject"))
        && let Ok(to_obj_fn) = to_obj_val.dyn_into::<js_sys::Function>()
        && let Ok(obj) = to_obj_fn.call0(val)
        && let Ok(s) = serde_wasm_bindgen::from_value::<ast::Stmt>(obj)
    {
        return Some(s);
    }
    serde_wasm_bindgen::from_value::<ast::Stmt>(val.clone()).ok()
}

/// Map a params binding to a query string via the shared typed contract.
pub(crate) fn bind_value_params(
    query: &str,
    params: &Value,
    truncate_vectors: bool,
) -> Result<String, JsValue> {
    qql_core::params_json::bind_str_with_values(query, params, truncate_vectors)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Map a params binding into a statement AST via the shared typed contract.
pub(crate) fn bind_stmt_values(stmt: &mut ast::Stmt, params: &Value) -> Result<(), JsValue> {
    qql_core::params_json::bind_stmt_with_values(stmt, params)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn maybe_bind(query: &str, params: Option<&Value>) -> Result<String, JsValue> {
    match params {
        Some(params) => bind_value_params(query, params, false),
        None => Ok(query.to_string()),
    }
}

fn invalid_params(message: impl Into<String>) -> QqlError {
    QqlError::validation("QQL-BIND-INVALID-PARAMS", message.into(), None)
}

/// Convert any JS value to a typed shard routing key (`None` clears).
///
/// Strings become keywords (empty clears), integers numeric keys — the same
/// split the reference parser enforces. `BigInt` carries exact integers of
/// any size; `number`s must be exact non-negative integers within the safe
/// range (larger keys need `BigInt`, never silent rounding). Booleans and
/// everything else fail closed.
pub(crate) fn jsvalue_to_shard_key(v: &JsValue) -> Result<Option<ast::ShardKey>, QqlError> {
    use qql_core::ast::ShardKey;
    if v.is_null() || v.is_undefined() {
        return Ok(None);
    }
    if let Some(s) = v.as_string() {
        return Ok(if s.is_empty() {
            None
        } else {
            Some(ShardKey::Keyword(s))
        });
    }
    if v.as_bool().is_some() {
        return Err(invalid_params(
            "shardKey must be a string, an integer, a BigInt, or null",
        ));
    }
    if let Ok(big) = v.clone().dyn_into::<js_sys::BigInt>() {
        let n = u64::try_from(big)
            .map_err(|_| invalid_params("shardKey BigInt does not fit in u64"))?;
        return Ok(Some(ShardKey::Number(n)));
    }
    if let Some(n) = v.as_f64() {
        if !n.is_finite() || n.fract() != 0.0 || n < 0.0 {
            return Err(invalid_params(
                "shardKey number must be a non-negative integer",
            ));
        }
        if n > 9007199254740991.0 {
            return Err(invalid_params(
                "shardKey number exceeds the exact-integer range; pass a BigInt instead",
            ));
        }
        return Ok(Some(ShardKey::Number(n as u64)));
    }
    Err(invalid_params(
        "shardKey must be a string, an integer, a BigInt, or null",
    ))
}

/// Convert any JS value to a typed [`Value`], mirroring `nqql-common`.
///
/// Scalars dispatch on `typeof` first so they never pay for typed-array
/// probing. `Float32Array` / `Float64Array` bind as packed `F32Array` in one
/// copy; `Int32Array` / `Uint32Array` bind as integer lists (sparse
/// `indices`). Raw `ArrayBuffer` / views without a float dtype and `BigInt`
/// fail closed with wrap-first guidance — the serde layer would otherwise
/// reject them as non-JSON values.
pub(crate) fn jsvalue_to_value(v: &JsValue) -> Result<Value, QqlError> {
    if v.is_null() || v.is_undefined() {
        return Ok(Value::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(Value::Bool(b));
    }
    if let Some(s) = v.as_string() {
        return Ok(Value::Str(s));
    }
    if let Some(n) = v.as_f64() {
        if !n.is_finite() {
            return Err(invalid_params("cannot bind non-finite float value"));
        }
        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
            return Ok(Value::Int(n as i64));
        }
        return Ok(Value::Float(n));
    }
    jsvalue_object_to_value(v)
}

fn typedarray_f64_to_value(items: &[f64]) -> Result<Value, QqlError> {
    let mut out = Vec::with_capacity(items.len());
    for x in items {
        if !x.is_finite() {
            return Err(invalid_params(
                "cannot bind non-finite float value in Float64Array",
            ));
        }
        out.push(*x as f32);
    }
    Ok(Value::F32Array(out))
}

fn jsvalue_object_to_value(v: &JsValue) -> Result<Value, QqlError> {
    // Typed arrays first: the serde layer rejects these as non-JSON values.
    // Each `dyn_into` validates the element type; order matters because every
    // view is also an object.
    if let Ok(a) = v.clone().dyn_into::<js_sys::Float64Array>() {
        return typedarray_f64_to_value(a.to_vec().as_slice());
    }
    if let Ok(a) = v.clone().dyn_into::<js_sys::Float32Array>() {
        return Ok(Value::F32Array(a.to_vec()));
    }
    if let Ok(a) = v.clone().dyn_into::<js_sys::Int32Array>() {
        return Ok(Value::List(
            a.to_vec()
                .iter()
                .map(|&i| Value::Int(i64::from(i)))
                .collect(),
        ));
    }
    if let Ok(a) = v.clone().dyn_into::<js_sys::Uint32Array>() {
        return Ok(Value::List(
            a.to_vec()
                .iter()
                .map(|&i| Value::Int(i64::from(i)))
                .collect(),
        ));
    }
    // Any other view (DataView, Uint8Array, …) or a raw ArrayBuffer carries
    // no float dtype the binder could honor — same fail-closed guidance as
    // the Node SDK.
    if js_sys::ArrayBuffer::is_view(v) || v.clone().dyn_into::<js_sys::ArrayBuffer>().is_ok() {
        return Err(invalid_params(
            "binary data must be wrapped in a Float32Array or Float64Array view first (e.g. new Float64Array(buffer))",
        ));
    }
    if js_sys::Array::is_array(v) {
        let arr = js_sys::Array::from(v);
        let len = arr.length() as usize;
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            let item = arr.get(i as u32);
            // Holes and explicit undefined read back as undefined.
            if item.is_undefined() {
                items.push(Value::Null);
            } else {
                items.push(jsvalue_to_value(&item)?);
            }
        }
        return Ok(Value::List(items));
    }
    if v.is_function() || v.is_symbol() || v.is_bigint() {
        return Err(invalid_params(
            "unsupported value type for parameter binding (expected bool, int, float, str, list, dict, Float32Array, or Float64Array)",
        ));
    }
    if let Some(obj) = js_sys::Object::try_from(v) {
        let mut entries = Vec::new();
        for key in js_sys::Object::keys(obj) {
            let Some(key_str) = key.as_string() else {
                continue;
            };
            let item = js_sys::Reflect::get(obj.as_ref(), &key)
                .map_err(|_| invalid_params("cannot read parameter object property"))?;
            // Object keys holding undefined are dropped (JSON semantics).
            if item.is_undefined() {
                continue;
            }
            entries.push((key_str, jsvalue_to_value(&item)?));
        }
        return Ok(Value::Dict(entries));
    }
    Err(invalid_params(
        "unsupported value type for parameter binding (expected bool, int, float, str, list, dict, Float32Array, or Float64Array)",
    ))
}

/// Parse the `batchSize` option (default 100). Values `< 1` fail closed with
/// the same code as every other SDK, before any I/O.
#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub(crate) fn batch_size_from(options: Option<&JsValue>) -> Result<usize, QqlError> {
    let Some(options) = options else {
        return Ok(100);
    };
    if options.is_null() || options.is_undefined() || !options.is_object() {
        return Ok(100);
    }
    let value = js_sys::Reflect::get(options, &JsValue::from_str("batchSize"))
        .map_err(|_| invalid_params("invalid options"))?;
    if value.is_undefined() || value.is_null() {
        return Ok(100);
    }
    match value.as_f64() {
        Some(n) if n.fract() == 0.0 && n >= 1.0 => Ok(n as usize),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-UPSERT-BATCH",
            "upsertMany batchSize must be >= 1",
            None,
        )),
    }
}
