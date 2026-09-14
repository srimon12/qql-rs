//! Request-level query parameters (`wait`, `timeout`, `consistency`).
//!
//! OpenAPI puts these on the URL, not the JSON body. The planner emits them
//! as `Route.query`; the converter inverts that by reading a wrapped
//! `"query"` object (or a `?…` suffix on `path`).

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, invalid};
use qql_core::ast::{ReadConsistency, SearchParams};

/// Query-string knobs recovered from a wrapped request.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RequestOpts {
    /// Mutation / index / quota `WAIT` flag (`?wait=`).
    pub wait: Option<bool>,
    /// Read timeout in seconds (`?timeout=`).
    pub timeout: Option<u64>,
    /// Read consistency (`?consistency=`).
    pub consistency: Option<ReadConsistency>,
}

impl RequestOpts {
    /// Decode the wrapped `"query"` member (object or query-string).
    pub(crate) fn from_value(value: Option<&Value>) -> Result<Self, ConvertError> {
        let Some(value) = value.filter(|v| !v.is_null()) else {
            return Ok(Self::default());
        };
        match value {
            Value::String(raw) => Self::from_query_string(raw),
            Value::Object(obj) => Self::from_query_object(obj, "query"),
            other => Err(invalid(
                "query",
                format!(
                    "expected a query object or query-string, got {}",
                    json::type_name(other)
                ),
            )),
        }
    }

    /// Decode `a=b&c=d` (the URI query, without `?`).
    pub(crate) fn from_query_string(raw: &str) -> Result<Self, ConvertError> {
        if raw.is_empty() {
            return Ok(Self::default());
        }
        let mut obj = json::Obj::new();
        for pair in raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((key, value)) => (key, value),
                None => (pair, ""),
            };
            obj.insert(key.to_string(), Value::String(value.to_string()));
        }
        Self::from_query_object(&obj, "query")
    }

    fn from_query_object(obj: &json::Obj, path: &str) -> Result<Self, ConvertError> {
        json::reject_unknown(obj, path, &["wait", "timeout", "consistency"])?;
        Ok(Self {
            wait: opt_bool_flag(obj, "wait", path)?,
            timeout: opt_timeout(obj, path)?,
            consistency: opt_consistency(obj, path)?,
        })
    }

    /// Overlay `other` on top of `self` (later values win when `Some`).
    pub(crate) fn merge(&mut self, other: Self) {
        if other.wait.is_some() {
            self.wait = other.wait;
        }
        if other.timeout.is_some() {
            self.timeout = other.timeout;
        }
        if other.consistency.is_some() {
            self.consistency = other.consistency;
        }
    }

    /// Copy timeout / consistency onto query `PARAMS`.
    pub(crate) fn apply_read(&self, params: &mut Option<SearchParams>) {
        if self.timeout.is_none() && self.consistency.is_none() {
            return;
        }
        let params = params.get_or_insert_with(SearchParams::default);
        if self.timeout.is_some() {
            params.timeout = self.timeout;
        }
        if self.consistency.is_some() {
            params.consistency = self.consistency.clone();
        }
    }

    /// Fail if this endpoint has no `WAIT` clause.
    pub(crate) fn reject_wait(&self) -> Result<(), ConvertError> {
        if self.wait.is_some() {
            Err(invalid(
                "query.wait",
                "wait has no QQL representation on this endpoint",
            ))
        } else {
            Ok(())
        }
    }

    /// Fail if this endpoint has no timeout / consistency `PARAMS`.
    pub(crate) fn reject_read(&self) -> Result<(), ConvertError> {
        if self.timeout.is_some() {
            return Err(invalid(
                "query.timeout",
                "timeout has no QQL representation on this endpoint",
            ));
        }
        if self.consistency.is_some() {
            return Err(invalid(
                "query.consistency",
                "consistency has no QQL representation on this endpoint",
            ));
        }
        Ok(())
    }
}

/// `timeout` as a JSON integer or a decimal string.
fn opt_timeout(obj: &json::Obj, path: &str) -> Result<Option<u64>, ConvertError> {
    match obj.get("timeout") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => raw
            .parse::<u64>()
            .map(Some)
            .map_err(|_| invalid(child(path, "timeout"), format!("invalid timeout '{raw}'"))),
        Some(value) => Ok(Some(json::u64_at(value, &child(path, "timeout"))?)),
    }
}

/// `wait` as a JSON bool or the strings `"true"` / `"false"`.
fn opt_bool_flag(obj: &json::Obj, key: &str, path: &str) -> Result<Option<bool>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(Value::String(raw)) if raw.eq_ignore_ascii_case("true") => Ok(Some(true)),
        Some(Value::String(raw)) if raw.eq_ignore_ascii_case("false") => Ok(Some(false)),
        Some(other) => Err(invalid(
            child(path, key),
            format!("expected a boolean, got {}", json::type_name(other)),
        )),
    }
}

/// `consistency` as a factor integer or majority/quorum/all.
fn opt_consistency(obj: &json::Obj, path: &str) -> Result<Option<ReadConsistency>, ConvertError> {
    let Some(value) = obj.get("consistency").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let field = child(path, "consistency");
    match value {
        Value::Number(n) => {
            let factor = n
                .as_u64()
                .ok_or_else(|| invalid(&field, "consistency factor must be an unsigned integer"))?;
            Ok(Some(ReadConsistency::Factor(factor)))
        }
        Value::String(raw) => parse_consistency(raw, &field).map(Some),
        other => Err(invalid(
            field,
            format!(
                "expected a consistency factor or majority/quorum/all, got {}",
                json::type_name(other)
            ),
        )),
    }
}

fn parse_consistency(raw: &str, path: &str) -> Result<ReadConsistency, ConvertError> {
    if let Ok(factor) = raw.parse::<u64>() {
        return Ok(ReadConsistency::Factor(factor));
    }
    match raw.to_ascii_lowercase().as_str() {
        "majority" => Ok(ReadConsistency::Majority),
        "quorum" => Ok(ReadConsistency::Quorum),
        "all" => Ok(ReadConsistency::All),
        _ => Err(invalid(
            path,
            format!("unknown consistency '{raw}' (expected majority, quorum, all, or a factor)"),
        )),
    }
}
