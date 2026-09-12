//! Decode OpenAPI `Filter` / `Condition` schemas into [`qql_core::ast::FilterExpr`].

use serde_json::Value;

use crate::ConvertError;
use crate::decode::condition::{field_condition, field_name, has_id, nested, slice};
use crate::json::{self, child, index, invalid};
use qql_core::ast::FilterExpr;

/// Decode an optional filter member (`"filter": {…}`).
///
/// An empty `{}` filter matches everything and yields `None` (QQL has no empty
/// `WHERE`); every other shape must decode fully or fail closed.
pub(crate) fn filter_field(
    obj: &json::Obj,
    key: &str,
    path: &str,
) -> Result<Option<Box<FilterExpr>>, ConvertError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(filter_opt(value, &child(path, key))?.map(Box::new)),
    }
}

/// Decode a filter that must select something (`NESTED`, corpus, selector).
pub(crate) fn filter(value: &Value, path: &str) -> Result<FilterExpr, ConvertError> {
    filter_opt(value, path)?
        .ok_or_else(|| invalid(path, "empty filters have no QQL representation here"))
}

/// Decode a `Filter` object; `None` for an object with no conditions.
pub(crate) fn filter_opt(value: &Value, path: &str) -> Result<Option<FilterExpr>, ConvertError> {
    let obj = json::object(value, path)?;
    // A bare `Condition` is a valid single-clause `Filter` on the wire
    // (`FilterExpression::Single`); nested filters lower to this form.
    const CONDITION_KEYS: [&str; 7] = [
        "key",
        "is_empty",
        "is_null",
        "has_id",
        "has_vector",
        "slice",
        "nested",
    ];
    if CONDITION_KEYS.iter().any(|key| obj.contains_key(*key)) {
        return Ok(Some(condition(value, path)?));
    }
    if let Some(min_should) = obj.get("min_should") {
        let min_path = child(path, "min_should");
        let min = json::object(min_should, &min_path)?;
        let _ = json::required(min, "min_count", &min_path)?;
        return Err(invalid(
            min_path,
            "min_should (at-least-N of a condition set) has no QQL representation",
        ));
    }
    for key in obj.keys() {
        match key.as_str() {
            "must" | "should" | "must_not" => {}
            other => {
                return Err(invalid(
                    child(path, other),
                    "unknown Filter field (expected must, should, must_not, or min_should)",
                ));
            }
        }
    }

    // Qdrant evaluates `must` / `must_not` with `all` and `should` with `any`
    // over the condition list. Empty `must` / `must_not` are vacuously true
    // (no constraint), while an empty `should` matches nothing — which QQL
    // cannot express, so it fails closed instead of widening the filter.
    let must = decode_list(obj, "must", path)?.filter(|list| !list.is_empty());
    let should = decode_list(obj, "should", path)?;
    let must_not = decode_list(obj, "must_not", path)?.filter(|list| !list.is_empty());
    if should.as_ref().is_some_and(Vec::is_empty) {
        return Err(invalid(
            child(path, "should"),
            "an empty `should` list matches no points and has no QQL representation",
        ));
    }

    let mut clauses: Vec<FilterExpr> = Vec::new();
    if let Some(must) = must {
        clauses.push(match must.len() {
            1 => must.into_iter().next().expect("len checked"),
            _ => FilterExpr::And { operands: must },
        });
    }
    if let Some(should) = should {
        clauses.push(match should.len() {
            1 => should.into_iter().next().expect("len checked"),
            _ => FilterExpr::Or { operands: should },
        });
    }
    if let Some(must_not) = must_not {
        let negated = match must_not.len() {
            1 => must_not.into_iter().next().expect("len checked"),
            _ => FilterExpr::Or { operands: must_not },
        };
        clauses.push(FilterExpr::Not {
            operand: Box::new(negated),
        });
    }

    Ok(match clauses.len() {
        0 => None,
        1 => clauses.into_iter().next(),
        _ => Some(FilterExpr::And { operands: clauses }),
    })
}

/// Decode a `must` / `should` / `must_not` member: one condition or a list.
fn decode_list(
    obj: &json::Obj,
    key: &str,
    path: &str,
) -> Result<Option<Vec<FilterExpr>>, ConvertError> {
    let Some(value) = obj.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let list_path = child(path, key);
    let items = match value {
        Value::Array(items) => items.clone(),
        single => vec![single.clone()],
    };
    let mut clauses = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        clauses.push(condition(item, &index(&list_path, i))?);
    }
    Ok(Some(clauses))
}

/// Decode one `Condition` value.
pub(crate) fn condition(value: &Value, path: &str) -> Result<FilterExpr, ConvertError> {
    let obj = json::object(value, path)?;

    if obj.contains_key("key") {
        return field_condition(obj, path);
    }
    let Some(key) = obj.keys().next() else {
        return Err(invalid(
            path,
            "empty Condition object (expected key/is_empty/is_null/has_id/has_vector/slice/nested)",
        ));
    };
    if obj.len() != 1 {
        return Err(invalid(
            path,
            "Condition object must contain exactly one variant",
        ));
    }
    match key.as_str() {
        "is_empty" => {
            let cond_path = child(path, "is_empty");
            let key_only = json::object(obj.get("is_empty").expect("key checked"), &cond_path)?;
            let key = field_name(key_only, &cond_path)?;
            Ok(FilterExpr::IsEmpty { field: key })
        }
        "is_null" => {
            let cond_path = child(path, "is_null");
            let key_only = json::object(obj.get("is_null").expect("key checked"), &cond_path)?;
            let key = field_name(key_only, &cond_path)?;
            Ok(FilterExpr::IsNull { field: key })
        }
        "has_id" => has_id(obj, path),
        "has_vector" => {
            let name = json::required(obj, "has_vector", path)
                .and_then(|v| Ok(json::string_at(v, &child(path, "has_vector"))?.to_string()))?;
            Ok(FilterExpr::HasVector { name })
        }
        "slice" => slice(obj, path),
        "nested" => nested(obj, path),
        "must" | "should" | "must_not" | "min_should" => filter(value, path),
        other => Err(invalid(child(path, other), "unknown Condition variant")),
    }
}
