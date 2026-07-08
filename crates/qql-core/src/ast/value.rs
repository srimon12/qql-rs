use alloc::borrow::Cow;

#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Value<'a> {
    Str(Cow<'a, str>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Dict(alloc::vec::Vec<(Cow<'a, str>, Value<'a>)>),
    List(alloc::vec::Vec<Value<'a>>),
}

impl<'a> core::fmt::Debug for Value<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Value::Str(s) => write!(f, "Str({:?})", s.as_ref()),
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
                    write!(f, "({:?}, {:?})", k.as_ref(), v)?;
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

impl<'a> Value<'a> {
    /// Returns the value for the last matching key in an insertion-ordered dict.
    ///
    /// QQL keeps dictionaries as vectors so parsed payloads retain source order
    /// for stable debug output and serialization. Duplicate keys are allowed at
    /// parse time; mutation treats the last key as authoritative.
    pub fn dict_get(&self, key: &str) -> Option<&Value<'a>> {
        match self {
            Value::Dict(items) => items
                .iter()
                .rev()
                .find(|(k, _)| k.as_ref() == key)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a key in an insertion-ordered dict, updating the last matching key
    /// or appending a new key when it is absent.
    pub fn dict_set(&mut self, key: Cow<'a, str>, value: Value<'a>) {
        if let Value::Dict(items) = self {
            if let Some((_, existing)) = items.iter_mut().rev().find(|(k, _)| k == &key) {
                *existing = value;
            } else {
                items.push((key, value));
            }
        }
    }

    pub fn to_static(&self) -> Value<'static> {
        match self {
            Value::Str(s) => Value::Str(Cow::Owned(s.to_string())),
            Value::Int(i) => Value::Int(*i),
            Value::Float(f) => Value::Float(*f),
            Value::Bool(b) => Value::Bool(*b),
            Value::Null => Value::Null,
            Value::Dict(items) => Value::Dict(
                items
                    .iter()
                    .map(|(k, v)| (Cow::Owned(k.to_string()), v.to_static()))
                    .collect(),
            ),
            Value::List(items) => Value::List(items.iter().map(|v| v.to_static()).collect()),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_ref()),
            _ => None,
        }
    }
}
