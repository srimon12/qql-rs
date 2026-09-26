//! AST `Value` wire serialization: JSON exactly once, at the boundary.
//!
//! Plan structs hold [`qql_core::ast::Value`] directly (payloads, match
//! values, inference objects, metadata). These helpers render them to the
//! OpenAPI JSON shape with byte-identical output to the removed eager
//! `value_to_json` conversion, so the contract suite pins no drift:
//! - `Str` → string, `Int`/`UInt` → integer, `Float` → number (`null` when
//!   non-finite — JSON has no inf/NaN), `Bool`/`Null` as-is.
//! - `List` → array, `Dict` → object (key order preserved as written).
//! - `F32Array` → array of numbers (same `f32 as f64` widening as before).
//! - `Param` / `PositionalParam` panic: `plan()` rejects unbound placeholders
//!   before lowering, mirroring the old invariant.
//!
//! Deserialization rides [`qql_core::ast::Value::from_json`] (objects become
//! `Dict`, arrays become `List`).

use serde::Deserialize as _;
use serde::Serialize as _;

use qql_core::ast::Value;

/// `serde` adapter borrowing an AST value with [`serialize_ast_value`] semantics.
pub(crate) struct SerValue<'a>(pub &'a Value);

impl serde::Serialize for SerValue<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_ast_value(self.0, serializer)
    }
}

/// `serde` adapter borrowing a boxed AST value.
pub(crate) struct SerValueBox<'a>(pub &'a alloc::boxed::Box<Value>);

impl serde::Serialize for SerValueBox<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_ast_value(self.0, serializer)
    }
}

/// `serde` adapter borrowing `Vec<(String, Value)>` pairs as a JSON object.
pub(crate) struct SerPairs<'a>(pub &'a alloc::vec::Vec<(alloc::string::String, Value)>);

impl serde::Serialize for SerPairs<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, &SerValue(value))?;
        }
        map.end()
    }
}

/// `serde` adapter borrowing optional pairs as a JSON object.
pub(crate) struct SerPairsOpt<'a>(pub &'a Option<alloc::vec::Vec<(alloc::string::String, Value)>>);

impl serde::Serialize for SerPairsOpt<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(pairs) => SerPairs(pairs).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

/// Serialize an AST value to its OpenAPI JSON shape.
pub(crate) fn serialize_ast_value<S: serde::Serializer>(
    value: &Value,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::{SerializeMap, SerializeSeq};
    match value {
        Value::Str(s) => serializer.serialize_str(s),
        Value::Int(n) => serializer.serialize_i64(*n),
        Value::UInt(n) => serializer.serialize_u64(*n),
        Value::Float(f) => match serde_json::Number::from_f64(*f) {
            Some(n) => n.serialize(serializer),
            None => serializer.serialize_unit(),
        },
        Value::Bool(b) => serializer.serialize_bool(*b),
        Value::Null => serializer.serialize_unit(),
        Value::F32Array(values) => {
            let mut seq = serializer.serialize_seq(Some(values.len()))?;
            for f in values {
                match serde_json::Number::from_f64(*f as f64) {
                    Some(n) => seq.serialize_element(&n)?,
                    None => seq.serialize_element(&())?,
                }
            }
            seq.end()
        }
        Value::List(items) => {
            let mut seq = serializer.serialize_seq(Some(items.len()))?;
            for item in items {
                seq.serialize_element(&SerValue(item))?;
            }
            seq.end()
        }
        Value::Dict(entries) => {
            let mut map = serializer.serialize_map(Some(entries.len()))?;
            for (key, item) in entries {
                map.serialize_entry(key, &SerValue(item))?;
            }
            map.end()
        }
        Value::Param(name, _) => {
            panic!("invariant violation: unbound parameter :{name} reached value serialization");
        }
        Value::PositionalParam(idx, _) => {
            panic!(
                "invariant violation: unbound positional parameter ?{idx} reached value serialization"
            );
        }
    }
}

/// Serialize `Option<Value>`, skipping nothing (caller adds
/// `skip_serializing_if`).
pub(crate) fn serialize_ast_value_opt<S: serde::Serializer>(
    value: &Option<Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serialize_ast_value(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// Serialize `Vec<Value>` as a JSON array.
pub(crate) fn serialize_ast_value_vec<S: serde::Serializer>(
    values: &alloc::vec::Vec<Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(values.len()))?;
    for value in values {
        seq.serialize_element(&SerValue(value))?;
    }
    seq.end()
}

/// Serialize `Vec<(String, Value)>` pairs as a JSON object.
pub(crate) fn serialize_ast_pairs<S: serde::Serializer>(
    pairs: &alloc::vec::Vec<(alloc::string::String, Value)>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    SerPairs(pairs).serialize(serializer)
}

/// Serialize `Option<Vec<(String, Value)>>` pairs as a JSON object.
pub(crate) fn serialize_ast_pairs_opt<S: serde::Serializer>(
    pairs: &Option<alloc::vec::Vec<(alloc::string::String, Value)>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    SerPairsOpt(pairs).serialize(serializer)
}

/// Serialize `Option<&Vec<(String, Value)>>` pairs as a JSON object (borrowed
/// REST projection views).
pub(crate) fn serialize_ast_pairs_opt_ref<S: serde::Serializer>(
    pairs: &Option<&alloc::vec::Vec<(alloc::string::String, Value)>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match pairs {
        Some(pairs) => SerPairs(pairs).serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// Deserialize the wire shape back into an AST value
/// (`Object` → `Dict`, `Array` → `List`; see [`Value::from_json`]).
pub(crate) fn deserialize_ast_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, D::Error> {
    let json = serde_json::Value::deserialize(deserializer)?;
    Value::from_json(json).map_err(serde::de::Error::custom)
}

/// Deserialize a JSON array into `Vec<Value>`.
pub(crate) fn deserialize_ast_value_vec<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<alloc::vec::Vec<Value>, D::Error> {
    let json = serde_json::Value::deserialize(deserializer)?;
    match json {
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(Value::from_json)
            .collect::<Result<_, _>>()
            .map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(alloc::format!(
            "expected a JSON array for match values, got {other}"
        ))),
    }
}

/// Render a scalar AST value exactly as the old `serde_json` pipeline did,
/// for error messages that pin value text (`"1.5"`, `"true"`,
/// `"9223372036854775808"`). Non-scalars fall back to AST debug shape;
/// only human-facing message text ever flows through here, never the wire.
pub fn value_error_text(value: &Value) -> alloc::string::String {
    match value {
        Value::Str(s) => serde_json::Value::String(s.clone()).to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".to_string()),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::F32Array(values) => {
            let rendered: alloc::vec::Vec<alloc::string::String> = values
                .iter()
                .map(|f| {
                    serde_json::Number::from_f64(*f as f64)
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "null".to_string())
                })
                .collect();
            alloc::format!("[{}]", rendered.join(", "))
        }
        other => alloc::format!("{other:?}"),
    }
}
