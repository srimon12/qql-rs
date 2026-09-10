//! Flat-float-list → [`Value::F32Array`] packing heuristic.
//!
//! `list[float]` parameters used to bind as a `Value::List` tree with one
//! `Value::Float` node per element. For vector-sized lists that is pure
//! overhead: a 384-d vector builds 384 `Value::Float` nodes only for the
//! planner to downcast every element to `f32` again. The binder cannot
//! distinguish a vector slot from a payload slot at conversion time — host
//! params are converted to a typed [`Value`] tree before
//! `bind_stmt_with_values` walks the AST, and the lookup contract
//! (`Fn(&str) -> Option<Value>`) carries no slot information — so the host
//! conversion applies a bounded heuristic instead: flat Python `float` lists
//! at or above [`FLOAT_LIST_F32_THRESHOLD`] elements pack as
//! [`Value::F32Array`] (the same representation the numpy / `array.array`
//! buffer path already produces), everything else keeps the per-element
//! [`Value::List`] contract with `f64` precision.
//!
//! Tradeoff: dense vectors are `f32` on every transport anyway, so vector
//! binds lose only the intermediate `f64` precision they never shipped;
//! long *payload* float lists (32+ elements) also take `f32` precision.
//! Nested lists (multi-dense vectors, payload matrices), int/bool lists, and
//! short float lists are untouched.

use qql_core::ast::Value;

/// Minimum flat-list length that packs as [`Value::F32Array`].
pub const FLOAT_LIST_F32_THRESHOLD: usize = 32;

/// Pack `values` into [`Value::F32Array`], rejecting any value that is not a
/// finite `f32` (non-finite input, or finite `f64` outside the `f32` range).
///
/// Returns the offending source value as `Err` so the caller can raise the
/// same non-finite binding error the per-element path raises.
pub fn pack_f32(values: &[f64]) -> Result<Value, f64> {
    let mut out = Vec::with_capacity(values.len());
    for &value in values {
        let packed = value as f32;
        if !value.is_finite() || !packed.is_finite() {
            return Err(value);
        }
        out.push(packed);
    }
    Ok(Value::F32Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_flat_float_sequence_as_f32_array() {
        let values: Vec<f64> = (0..384).map(|i| i as f64 * 0.001).collect();
        let packed = pack_f32(&values).expect("finite values pack");
        let Value::F32Array(out) = packed else {
            panic!("flat float sequence must pack as Value::F32Array");
        };
        assert_eq!(out.len(), 384);
        assert_eq!(out[0], 0.0f32);
        assert_eq!(out[383], (383.0f64 * 0.001) as f32);
        // f32 precision is the contract: 1.0000000001 rounds to 1.0.
        assert_eq!(
            pack_f32(&[1.0000000001_f64]).unwrap(),
            Value::F32Array(vec![1.0])
        );
    }

    #[test]
    fn rejects_non_finite_and_f32_overflow() {
        // `NaN != NaN`, so assert on the error discriminant and, where
        // comparable, the returned offending value.
        for bad in [f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(pack_f32(&[bad]), Err(bad));
        }
        assert!(matches!(pack_f32(&[f64::NAN]), Err(v) if v.is_nan()));
        // Finite f64 beyond f32 range cannot be represented; fail closed
        // instead of shipping infinity to the backend.
        let huge = f64::MAX;
        assert_eq!(pack_f32(&[huge]), Err(huge));
    }

    #[test]
    fn threshold_is_32() {
        assert_eq!(FLOAT_LIST_F32_THRESHOLD, 32);
    }
}
