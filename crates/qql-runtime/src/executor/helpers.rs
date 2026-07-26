use qql_core::ast::Value;

pub(crate) fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => {
            serde_json::Value::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| {
                serde_json::Number::from_f64(0.0).unwrap_or(serde_json::Number::from(0))
            }))
        }
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::Dict(items) => {
            let mut map = serde_json::Map::new();
            for (k, v) in items {
                map.insert(k.to_string(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
    }
}
