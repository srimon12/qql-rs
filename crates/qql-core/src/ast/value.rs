#[cfg(feature = "json")]
use crate::error::QqlError;
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
            Self::Str(value) => f.debug_tuple("Str").field(value).finish(),
            Self::Int(value) => f.debug_tuple("Int").field(value).finish(),
            Self::Float(value) => f.debug_tuple("Float").field(value).finish(),
            Self::Bool(value) => f.debug_tuple("Bool").field(value).finish(),
            Self::Null => f.write_str("Null"),
            Self::Dict(value) => f.debug_tuple("Dict").field(value).finish(),
            Self::List(value) => f.debug_tuple("List").field(value).finish(),
        }
    }
}

impl Value {
    pub fn dict_get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(items) => items
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn dict_set(&mut self, key: String, value: Value) {
        if let Self::Dict(items) = self {
            if let Some((_, current)) = items
                .iter_mut()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(&key))
            {
                *current = value;
            } else {
                items.push((key, value));
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    #[cfg(feature = "json")]
    pub fn from_json(value: serde_json::Value) -> Result<Self, QqlError> {
        match value {
            serde_json::Value::String(value) => Ok(Self::Str(value)),
            serde_json::Value::Number(value) => value
                .as_i64()
                .map(Self::Int)
                .or_else(|| value.as_f64().map(Self::Float))
                .ok_or_else(|| {
                    QqlError::validation(
                        "QQL-JSON-NUMBER",
                        "JSON number cannot be represented by QQL",
                        None,
                    )
                }),
            serde_json::Value::Bool(value) => Ok(Self::Bool(value)),
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Array(items) => items
                .into_iter()
                .map(Self::from_json)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            serde_json::Value::Object(items) => items
                .into_iter()
                .map(|(key, value)| Self::from_json(value).map(|value| (key, value)))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Dict),
        }
    }

    #[cfg(feature = "json")]
    pub fn to_json(&self) -> Result<serde_json::Value, QqlError> {
        match self {
            Self::Str(value) => Ok(serde_json::Value::String(value.clone())),
            Self::Int(value) => Ok(serde_json::Value::Number((*value).into())),
            Self::Float(value) => serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .ok_or_else(|| {
                    QqlError::validation(
                        "QQL-JSON-NONFINITE",
                        "non-finite floats cannot be converted to JSON",
                        None,
                    )
                }),
            Self::Bool(value) => Ok(serde_json::Value::Bool(*value)),
            Self::Null => Ok(serde_json::Value::Null),
            Self::Dict(items) => {
                let mut object = serde_json::Map::new();
                for (key, value) in items {
                    object.insert(key.clone(), value.to_json()?);
                }
                Ok(serde_json::Value::Object(object))
            }
            Self::List(items) => items
                .iter()
                .map(Self::to_json)
                .collect::<Result<Vec<_>, _>>()
                .map(serde_json::Value::Array),
        }
    }
}
