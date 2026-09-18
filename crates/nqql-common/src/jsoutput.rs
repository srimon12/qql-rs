//! Exact-integer JS output for the NAPI bindings.
//!
//! One uniform rule for every JS-facing JSON value (reports, Stmt objects,
//! routes, tokens): integers a JS `Number` holds exactly stay numbers;
//! anything larger (Qdrant snowflake u64 point IDs) crosses as `BigInt`.
//! This mirrors `qql-wasm`'s `json_value_to_js` + `classify_number` so the
//! two SDKs agree on the JS type of every value.
//!
//! Without this, report JSON text round-trips through `JSON.parse` and every
//! u64 above 2^53 silently rounds (`1479834607549681654` → `...700`).

use napi::bindgen_prelude::{BigInt, Env, Null, Object, ToNapiValue};

/// Largest-magnitude integers a JS `Number` holds exactly. Larger ones must
/// cross as `BigInt`; silently rounding them would mistarget points.
pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;

/// How one JSON number maps onto JS: exactly, or not at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntegerMapping {
    /// Fits a JS `Number` exactly — stays a number (full back-compat for
    /// counts, spans, limits, small IDs and payloads).
    Safe(f64),
    /// Unsigned snowflake (Qdrant u64 point IDs) — must be a `BigInt`.
    BigU(u64),
    /// Negative beyond the safe range — must be a `BigInt`.
    BigI(i64),
    /// Non-integer JSON number — a JS `Number` as always.
    Float(f64),
}

/// Classify one JSON number for the JS boundary. Pure over
/// `serde_json::Number`, so the boundary rule is unit-testable without a JS
/// runtime; the `ToNapiValue` construction itself is a trivial follow-on.
pub fn classify_number(raw: &serde_json::Number) -> IntegerMapping {
    if let Some(signed) = raw.as_i64() {
        if (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&signed) {
            return IntegerMapping::Safe(signed as f64);
        }
        // Anything that fits `i64` but not the safe range keeps its sign.
        return IntegerMapping::BigI(signed);
    }
    if let Some(unsigned) = raw.as_u64() {
        // Reached only above `i64::MAX`: always beyond the safe range.
        return IntegerMapping::BigU(unsigned);
    }
    IntegerMapping::Float(raw.as_f64().unwrap_or(f64::NAN))
}

/// Borrowed JSON view mapped with the boundary rule; the `ToNapiValue` impl
/// below recurses through arrays/objects with this, so containers need no
/// per-field mapping.
struct Exact<'a>(&'a serde_json::Value);

impl ToNapiValue for Exact<'_> {
    unsafe fn to_napi_value(
        env: napi::sys::napi_env,
        val: Self,
    ) -> napi::Result<napi::sys::napi_value> {
        // SAFETY: napi invokes this synchronously on the JS thread with a
        // valid env (trait contract). Every conversion below uses napi's own
        // safe `ToNapiValue` wrappers or `Object`/`Vec` builders; the only
        // raw calls are the same wrappers' audited internals. No handles
        // escape: the returned `napi_value` is owned by the JS call result.
        unsafe { exact_to_napi_value(env, val.0) }
    }
}

unsafe fn exact_to_napi_value(
    env: napi::sys::napi_env,
    value: &serde_json::Value,
) -> napi::Result<napi::sys::napi_value> {
    // SAFETY: same contract as the `ToNapiValue` caller above — valid env on
    // the JS thread; only safe napi builders run, and nothing escapes except
    // the returned handle.
    match value {
        serde_json::Value::Null => unsafe { Null::to_napi_value(env, Null) },
        serde_json::Value::Bool(flag) => unsafe { ToNapiValue::to_napi_value(env, flag) },
        serde_json::Value::Number(raw) => match classify_number(raw) {
            IntegerMapping::Safe(number) | IntegerMapping::Float(number) => unsafe {
                ToNapiValue::to_napi_value(env, number)
            },
            IntegerMapping::BigU(unsigned) => unsafe {
                BigInt::to_napi_value(env, BigInt::from(unsigned))
            },
            IntegerMapping::BigI(signed) => unsafe {
                BigInt::to_napi_value(env, BigInt::from(signed))
            },
        },
        serde_json::Value::String(text) => unsafe { ToNapiValue::to_napi_value(env, text) },
        serde_json::Value::Array(items) => {
            let nested: Vec<Exact> = items.iter().map(Exact).collect();
            unsafe { ToNapiValue::to_napi_value(env, nested) }
        }
        serde_json::Value::Object(fields) => {
            let mut object = Object::new(&Env::from(env))?;
            for (key, item) in fields {
                object.set(key, Exact(item))?;
            }
            unsafe { Object::to_napi_value(env, object) }
        }
    }
}

/// Owned JSON value mapped onto JS with the boundary rule: safe integers as
/// `Number`, unrepresentable ones as `BigInt`.
///
/// Return this (instead of a JSON string the wrapper would `JSON.parse` —
/// which rounds every u64 through f64) from every `#[napi]` entry point that
/// answers JSON: execute, analyze, upsert, Stmt objects, routes, tokens.
pub struct BigIntSafeJson(pub serde_json::Value);

impl ToNapiValue for BigIntSafeJson {
    unsafe fn to_napi_value(
        env: napi::sys::napi_env,
        val: Self,
    ) -> napi::Result<napi::sys::napi_value> {
        // SAFETY: as for `Exact` — valid env on the JS thread, safe builders
        // only, and only the result handle escapes.
        unsafe { exact_to_napi_value(env, &val.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(json: serde_json::Value) -> IntegerMapping {
        let number = match &json {
            serde_json::Value::Number(raw) => raw.clone(),
            other => panic!("expected a JSON number, got {other}"),
        };
        classify_number(&number)
    }

    #[test]
    fn safe_integers_stay_numbers() {
        for (json, expected) in [
            (serde_json::json!(0), 0.0),
            (serde_json::json!(7), 7.0),
            (serde_json::json!(-3), -3.0),
            (serde_json::json!(9007199254740991i64), 9007199254740991.0),
            (serde_json::json!(-9007199254740991i64), -9007199254740991.0),
        ] {
            assert_eq!(classify(json), IntegerMapping::Safe(expected));
        }
    }

    #[test]
    fn snowflake_ids_map_to_bigint_exactly() {
        // Real geosmart hit: must survive the boundary without rounding.
        assert_eq!(
            classify(serde_json::json!(1479834607549681654u64)),
            IntegerMapping::BigI(1479834607549681654)
        );
        // Just past the safe range, both signs.
        assert_eq!(
            classify(serde_json::json!(9007199254740992u64)),
            IntegerMapping::BigI(9007199254740992)
        );
        assert_eq!(
            classify(serde_json::json!(-9007199254740992i64)),
            IntegerMapping::BigI(-9007199254740992)
        );
        // Above `i64::MAX` keeps the unsigned value.
        assert_eq!(
            classify(serde_json::Value::Number(serde_json::Number::from(
                u64::MAX
            ))),
            IntegerMapping::BigU(u64::MAX)
        );
    }

    #[test]
    fn floats_pass_through_untouched() {
        assert_eq!(
            classify(serde_json::json!(0.95)),
            IntegerMapping::Float(0.95)
        );
        assert_eq!(
            classify(serde_json::json!(1e21)),
            IntegerMapping::Float(1e21)
        );
    }
}
