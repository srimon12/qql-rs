use serde::{Deserialize, Serialize};
use std::boxed::Box;
use std::string::String;
use std::vec::Vec;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QdrantFilter {
    pub must: Option<Vec<QdrantCondition>>,
    pub must_not: Option<Vec<QdrantCondition>>,
    pub should: Option<Vec<QdrantCondition>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QdrantCondition {
    Match {
        key: String,
        value: FilterValue,
    },
    MatchExcept {
        key: String,
        value: FilterValue,
    },
    MatchKeywords {
        key: String,
        values: Vec<FilterValue>,
    },
    MatchExceptKeywords {
        key: String,
        values: Vec<FilterValue>,
    },
    Range {
        key: String,
        gt: Option<f64>,
        gte: Option<f64>,
        lt: Option<f64>,
        lte: Option<f64>,
    },
    IsNull {
        key: String,
    },
    IsEmpty {
        key: String,
    },
    Nested {
        key: String,
        filter: Box<QdrantFilter>,
    },
    HasId(Vec<FilterValue>),
    Boolean(Box<QdrantFilter>),
    MatchText {
        key: String,
        text: String,
    },
    MatchAny {
        key: String,
        text: String,
    },
    MatchPhrase {
        key: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl FilterValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FilterValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            FilterValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FilterValue::Float(f) => Some(*f),
            FilterValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            FilterValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}
