use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Dict(Vec<(String, Value)>),
    List(Vec<Value>),
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Value::Str(s) => write!(f, "Str({:?})", s.as_str()),
            Value::Int(i) => write!(f, "Int({})", i),
            Value::Float(fl) => write!(f, "Float({:?})", fl),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::Null => write!(f, "Null"),
            Value::Dict(d) => {
                f.write_str("Dict([")?;
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "({:?}, {:?})", k.as_str(), v)?;
                }
                f.write_str("])")
            }
            Value::List(l) => {
                f.write_str("List([")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                f.write_str("])")
            }
        }
    }
}

impl Value {
    /// Returns the value for the last matching key in an insertion-ordered dict.
    ///
    /// QQL keeps dictionaries as vectors so parsed payloads retain source order
    /// for stable debug output and serialization. Duplicate keys are allowed at
    /// parse time; mutation treats the last key as authoritative.
    pub fn dict_get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(items) => items
                .iter()
                .rev()
                .find(|(k, _)| k.as_str() == key)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a key in an insertion-ordered dict, updating the last matching key
    /// or appending a new key when it is absent.
    pub fn dict_set(&mut self, key: String, value: Value) {
        if let Value::Dict(items) = self {
            if let Some((_, existing)) = items.iter_mut().rev().find(|(k, _)| *k == key) {
                *existing = value;
            } else {
                items.push((key, value));
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Converts all Cow-borrowed strings to owned strings.
    /// This is a no-op since Value is already fully owned.
    #[inline]
    pub fn to_static(&self) -> Value {
        self.clone()
    }

    /// Converts a `serde_json::Value` into a QQL `Value`.
    ///
    /// Supports the same tagged-object format as the SDK bindings:
    /// `{"str": "..."}`, `{"int": 1}`, `{"float": 1.0}`, `{"bool": true}`,
    /// `{"null": null}`, `{"list": [...]}`, `{"dict": {...}}`.
    /// Untagged objects are treated as dicts.
    #[cfg(feature = "serde")]
    pub fn from_json(jv: serde_json::Value) -> Option<Value> {
        match jv {
            serde_json::Value::String(s) => Some(Value::Str(s)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(Value::Int(i))
                } else {
                    n.as_f64().map(Value::Float)
                }
            }
            serde_json::Value::Bool(b) => Some(Value::Bool(b)),
            serde_json::Value::Null => Some(Value::Null),
            serde_json::Value::Array(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(Value::from_json(item)?);
                }
                Some(Value::List(vals))
            }
            serde_json::Value::Object(map) => {
                if map.len() == 1 {
                    if let Some((tag, inner)) = map.iter().next() {
                        match tag.as_str() {
                            "str" => return inner.as_str().map(|s| Value::Str(s.to_string())),
                            "int" => return inner.as_i64().map(Value::Int),
                            "float" => return inner.as_f64().map(Value::Float),
                            "bool" => return inner.as_bool().map(Value::Bool),
                            "null" if inner.is_null() => return Some(Value::Null),
                            "list" => return Value::from_json(inner.clone()),
                            "dict" => return Value::from_json(inner.clone()),
                            _ => {}
                        }
                    }
                }
                let mut pairs = Vec::with_capacity(map.len());
                for (k, v) in map {
                    pairs.push((k, Value::from_json(v)?));
                }
                Some(Value::Dict(pairs))
            }
        }
    }

    #[cfg(feature = "serde")]
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Str(s) => serde_json::Value::String(s.clone()),
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
                    map.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(map)
            }
            Value::List(items) => {
                serde_json::Value::Array(items.iter().map(|item| item.to_json()).collect())
            }
        }
    }
}
