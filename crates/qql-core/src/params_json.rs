//! Host-language parameter binding over JSON-shaped `params` values.
//!
//! Every binding (PyO3, NAPI-RS, wasm-bindgen) receives parameters as
//! JSON-shaped data — a Python dict, a JS object, a `JsValue` object. This
//! module is the single source of truth for how such a value binds into a
//! statement or a query string, so the SDKs cannot drift:
//!
//! - an **object** binds named `:name` parameters (nested objects expand to
//!   dotted keys: `{"loc": {"lat": 1}}` binds `:loc.lat`);
//! - an **array** binds positional `?` parameters;
//! - anything else is rejected (`QQL-BIND-INVALID-PARAMS`).
//!
//! Batch dispatch is centralized in `plan_statement_params`: a params array
//! whose entries are all objects or arrays is a *statement-scoped* list (one
//! params container per statement, length must match — `QQL-BIND-BATCH-LENGTH`
//! otherwise); every other shape applies to every statement identically.

use crate::ast::Value;
use crate::ast::statement::Stmt;
use crate::error::QqlError;
use crate::params::{bind_named, bind_named_readable, bind_positional, bind_positional_readable};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec::Vec;

/// Flatten a JSON object into dotted parameter keys.
///
/// `{"loc": {"lat": 1}}` yields both the parent key `loc` (as a dict value)
/// and the dotted key `loc.lat`, so `:loc` and `:loc.lat` both resolve.
/// Flat dotted keys (`{"loc.lat": 1}`) work as well.
pub fn flatten_object(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<BTreeMap<String, Value>, QqlError> {
    let mut out = BTreeMap::new();
    flatten_into(obj, "", &mut out)?;
    Ok(out)
}

fn flatten_into(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    out: &mut BTreeMap<String, Value>,
) -> Result<(), QqlError> {
    for (k, v) in obj {
        let full_key = if prefix.is_empty() {
            k.clone()
        } else {
            alloc::format!("{prefix}.{k}")
        };
        if let serde_json::Value::Object(nested) = v {
            flatten_into(nested, &full_key, out)?;
        }
        out.insert(full_key, Value::from_json(v.clone())?);
    }
    Ok(())
}

/// Bind a JSON-shaped `params` value into a single statement AST in-place.
pub fn bind_stmt_with_params(stmt: &mut Stmt, params: &serde_json::Value) -> Result<(), QqlError> {
    match params {
        serde_json::Value::Object(obj) => {
            let map = flatten_object(obj)?;
            crate::params::bind_stmt(stmt, |k| map.get(k).cloned(), &[])
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr
                .iter()
                .cloned()
                .map(Value::from_json)
                .collect::<Result<Vec<_>, _>>()?;
            crate::params::bind_stmt(stmt, |_| None, &items)
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-PARAMS",
            "params must be an object for named parameters (:name) or an array for positional parameters (?)",
            None,
        )),
    }
}

/// Bind a JSON-shaped `params` value into a query string.
///
/// With `truncate_vectors`, long vector literals render as compact
/// `[0.1, 0.2, ... (N dims)]` previews.
pub fn bind_str_with_params(
    query: &str,
    params: &serde_json::Value,
    truncate_vectors: bool,
) -> Result<String, QqlError> {
    match params {
        serde_json::Value::Object(obj) => {
            let map = flatten_object(obj)?;
            if truncate_vectors {
                bind_named_readable(query, |k| map.get(k).cloned(), 2)
            } else {
                bind_named(query, |k| map.get(k).cloned())
            }
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr
                .iter()
                .cloned()
                .map(Value::from_json)
                .collect::<Result<Vec<_>, _>>()?;
            if truncate_vectors {
                bind_positional_readable(query, &items, 2)
            } else {
                bind_positional(query, &items)
            }
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-PARAMS",
            "params must be an object for named parameters (:name) or an array for positional parameters (?)",
            None,
        )),
    }
}

/// How a `params` argument applies to a batch of statements.
#[derive(Debug)]
pub enum ParamPlan<'a> {
    /// The same params value binds every statement (named object or shared
    /// positional list).
    Shared(&'a serde_json::Value),
    /// One params container per statement; length is guaranteed to match the
    /// statement count. Entries are objects (named) or arrays (positional).
    Scoped(&'a [serde_json::Value]),
}

/// Decide how `params` applies to `stmt_count` statements.
///
/// A non-empty array whose entries are **all** objects or arrays is treated as
/// statement-scoped: entry *i* binds statement *i*. The length must match the
/// statement count exactly, otherwise `QQL-BIND-BATCH-LENGTH` is raised —
/// silent partial binding is the bug class this contract exists to prevent.
///
/// Any other shape (object, scalar array, scalar) applies identically to every
/// statement: `params=[1, 2]` is a shared positional list, not per-statement.
pub fn plan_statement_params(
    params: &serde_json::Value,
    stmt_count: usize,
) -> Result<ParamPlan<'_>, QqlError> {
    if let serde_json::Value::Array(arr) = params
        && !arr.is_empty()
        && arr.iter().all(|p| p.is_object() || p.is_array())
    {
        if arr.len() == stmt_count {
            return Ok(ParamPlan::Scoped(arr));
        }
        return Err(QqlError::validation(
            "QQL-BIND-BATCH-LENGTH",
            format!(
                "statement-scoped params list has {} entr{} but {} statement{} given; provide one params container (object or array) per statement",
                arr.len(),
                if arr.len() == 1 { "y" } else { "ies" },
                stmt_count,
                if stmt_count == 1 { " was" } else { "s were" }
            ),
            None,
        ));
    }
    Ok(ParamPlan::Shared(params))
}

/// Resolve the params container for statement `index` under `plan`.
pub fn param_for<'a>(plan: &'a ParamPlan<'a>, index: usize) -> &'a serde_json::Value {
    match plan {
        ParamPlan::Shared(params) => params,
        ParamPlan::Scoped(list) => &list[index],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use serde_json::json;

    #[test]
    fn flatten_expands_nested_objects_to_dotted_keys() {
        let map =
            flatten_object(json!({"loc": {"lat": 1}, "top": 2}).as_object().unwrap()).unwrap();
        assert_eq!(map["top"], Value::Int(2));
        assert_eq!(map["loc.lat"], Value::Int(1));
        assert!(map.contains_key("loc"));
    }

    #[test]
    fn bind_stmt_rejects_invalid_params_shape() {
        let mut stmt = Parser::parse("QUERY [0.1] FROM docs WHERE x = :x").unwrap();
        let err = bind_stmt_with_params(&mut stmt, &json!("scalar")).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-INVALID-PARAMS");
    }

    #[test]
    fn bind_str_named_positional_and_truncation() {
        let q = "QUERY TEXT :q FROM docs WHERE t = ? LIMIT ?";
        let bound = bind_str_with_params(q, &json!({"q": "x"}), false).unwrap_err();
        assert_eq!(bound.code, "QQL-BIND-MIXED-STYLE");

        let bound =
            bind_str_with_params("QUERY TEXT :q FROM docs", &json!({"q": "x"}), false).unwrap();
        assert_eq!(bound, "QUERY TEXT 'x' FROM docs");

        let bound = bind_str_with_params("QUERY TEXT ? FROM docs", &json!(["a"]), false).unwrap();
        assert_eq!(bound, "QUERY TEXT 'a' FROM docs");
    }

    #[test]
    fn plan_scoped_requires_exact_length() {
        let params = json!([{"a": 1}, {"b": 2}]);
        assert!(matches!(
            plan_statement_params(&params, 2).unwrap(),
            ParamPlan::Scoped(_)
        ));
        let err = plan_statement_params(&params, 3).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-BATCH-LENGTH");

        // Scalars are shared positional values, never per-statement.
        let shared = json!([1, 2]);
        match plan_statement_params(&shared, 2).unwrap() {
            ParamPlan::Shared(_) => {}
            ParamPlan::Scoped(_) => panic!("scalar arrays must not scope"),
        }

        // Empty arrays bind nothing, shared.
        match plan_statement_params(&json!([]), 1).unwrap() {
            ParamPlan::Shared(_) => {}
            ParamPlan::Scoped(_) => panic!("empty arrays must not scope"),
        }

        // A single statement scoped with one container is allowed.
        let single = json!([{"q": "x"}]);
        assert!(matches!(
            plan_statement_params(&single, 1).unwrap(),
            ParamPlan::Scoped(_)
        ));
    }

    #[test]
    fn bind_batch_end_to_end() {
        let mut stmts = Parser::parse_all(
            "QUERY [0.1] FROM docs WHERE x = :x LIMIT :lim; QUERY [0.2] FROM docs WHERE y = :y;",
        )
        .unwrap();
        let params = json!([{"x": 1, "lim": 5}, {"y": "k"}]);
        let plan = plan_statement_params(&params, stmts.len()).unwrap();
        for (i, stmt) in stmts.iter_mut().enumerate() {
            bind_stmt_with_params(stmt, param_for(&plan, i)).unwrap();
        }
        assert_eq!(
            crate::fmt::format_stmt(&stmts[0]),
            "QUERY VECTOR [0.1] FROM docs WHERE x = 1 LIMIT 5"
        );
        assert_eq!(
            crate::fmt::format_stmt(&stmts[1]),
            "QUERY VECTOR [0.2] FROM docs WHERE y = 'k'"
        );
    }
}
