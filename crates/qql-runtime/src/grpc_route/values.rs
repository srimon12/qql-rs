//! Payload value conversions: AST [`qql_core::ast::Value`] ↔ proto [`qdrant::Value`].
//!
//! The plan holds AST values (rendered to JSON only for REST); the gRPC path
//! converts them straight to proto with no JSON detour.

use qql_core::ast::Value;

use crate::qdrant_grpc::qdrant;

/// Convert a borrowed AST value to its proto form (one string/container copy
/// per node; the gratuitous whole-subtree pre-clone is gone).
pub(crate) fn to_qdrant_value(val: &Value) -> qdrant::Value {
    use qdrant::value::Kind;
    match val {
        Value::Null => qdrant::Value { kind: None },
        Value::Bool(b) => qdrant::Value {
            kind: Some(Kind::BoolValue(*b)),
        },
        Value::Int(n) => qdrant::Value {
            kind: Some(Kind::IntegerValue(*n)),
        },
        Value::UInt(n) => {
            if let Ok(i) = i64::try_from(*n) {
                qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                // Mirror the old JSON pipeline (`Number::as_i64` fails, falls
                // back to double): no silent wrap into a negative integer.
                qdrant::Value {
                    kind: Some(Kind::DoubleValue(*n as f64)),
                }
            }
        }
        Value::Float(f) => qdrant::Value {
            kind: Some(Kind::DoubleValue(*f)),
        },
        Value::Str(s) => qdrant::Value {
            kind: Some(Kind::StringValue(s.clone())),
        },
        Value::F32Array(values) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: values
                    .iter()
                    .map(|f| qdrant::Value {
                        kind: Some(Kind::DoubleValue(*f as f64)),
                    })
                    .collect(),
            })),
        },
        Value::List(items) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: items.iter().map(to_qdrant_value).collect(),
            })),
        },
        Value::Dict(entries) => {
            let fields = entries
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v)))
                .collect();
            qdrant::Value {
                kind: Some(Kind::StructValue(qdrant::Struct { fields })),
            }
        }
        Value::Param(name, _) => {
            panic!("invariant violation: unbound parameter :{name} reached gRPC value conversion");
        }
        Value::PositionalParam(idx, _) => {
            panic!(
                "invariant violation: unbound positional parameter ?{idx} reached gRPC value conversion"
            );
        }
    }
}

// ── Proto response → JSON conversion ─────────────────────────────

pub(crate) fn qdrant_value_to_json(v: &qdrant::Value) -> serde_json::Value {
    use qdrant::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::DoubleValue(d)) => serde_json::json!(*d),
        Some(Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::BoolValue(b)) => serde_json::json!(*b),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), qdrant_value_to_json(v)))
                .collect(),
        ),
    }
}
