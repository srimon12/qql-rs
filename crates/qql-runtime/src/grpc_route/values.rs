//! Payload value conversions: JSON ↔ proto [`qdrant::Value`].

use crate::qdrant_grpc::qdrant;

pub(crate) fn to_qdrant_value(val: serde_json::Value) -> qdrant::Value {
    use qdrant::value::Kind;
    match val {
        serde_json::Value::Null => qdrant::Value { kind: None },
        serde_json::Value::Bool(b) => qdrant::Value {
            kind: Some(Kind::BoolValue(b)),
        },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                qdrant::Value {
                    kind: Some(Kind::DoubleValue(n.as_f64().unwrap_or(0.0))),
                }
            }
        }
        serde_json::Value::String(s) => qdrant::Value {
            kind: Some(Kind::StringValue(s)),
        },
        serde_json::Value::Array(arr) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: arr.into_iter().map(to_qdrant_value).collect(),
            })),
        },
        serde_json::Value::Object(obj) => {
            let fields = obj
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect();
            qdrant::Value {
                kind: Some(Kind::StructValue(qdrant::Struct { fields })),
            }
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
