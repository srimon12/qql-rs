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
//!
//! Keep this module in lockstep with [`crate::params`]: named/positional
//! flattening, duplicate-key rejection, and bind errors must match the typed
//! `Value` path. The only intended deltas are JSON-specific spellings
//! (`F32Array` fast path and `{"data","dim"}` multivector).

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
        let parsed_val = Value::from_json(v.clone())?;
        if out.contains_key(&full_key) {
            return Err(QqlError::validation(
                "QQL-BIND-DUPLICATE-PARAM",
                alloc::format!(
                    "duplicate parameter key '{full_key}' from flat and nested parameter sources"
                ),
                None,
            ));
        }
        out.insert(full_key, parsed_val);
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

/// Flatten a [`Value`] object into dotted parameter keys.
///
/// Same contract as [`flatten_object`], but over an already-typed [`Value`]
/// tree — the entry point for host bindings (NAPI typed arrays, Python
/// buffer-protocol objects) that convert FFI values straight to [`Value`],
/// bypassing the JSON round-trip. `F32Array` values flow through untouched so
/// dense vectors bind without per-element boxing.
pub fn flatten_value_object(obj: &[(String, Value)]) -> Result<BTreeMap<String, Value>, QqlError> {
    let mut out = BTreeMap::new();
    flatten_value_into(obj, "", &mut out)?;
    Ok(out)
}

fn flatten_value_into(
    obj: &[(String, Value)],
    prefix: &str,
    out: &mut BTreeMap<String, Value>,
) -> Result<(), QqlError> {
    for (k, v) in obj {
        let full_key = if prefix.is_empty() {
            k.clone()
        } else {
            alloc::format!("{prefix}.{k}")
        };
        if let Value::Dict(nested) = v {
            flatten_value_into(nested, &full_key, out)?;
        }
        if out.contains_key(&full_key) {
            return Err(QqlError::validation(
                "QQL-BIND-DUPLICATE-PARAM",
                alloc::format!(
                    "duplicate parameter key '{full_key}' from flat and nested parameter sources"
                ),
                None,
            ));
        }
        out.insert(full_key, v.clone());
    }
    Ok(())
}

/// Bind a typed [`Value`] `params` into a single statement AST in-place.
///
/// Same contract as [`bind_stmt_with_params`], minus the JSON layer.
pub fn bind_stmt_with_values(stmt: &mut Stmt, params: &Value) -> Result<(), QqlError> {
    match params {
        Value::Dict(obj) => {
            let map = flatten_value_object(obj)?;
            crate::params::bind_stmt(stmt, |k| map.get(k).cloned(), &[])
        }
        Value::List(arr) => crate::params::bind_stmt(stmt, |_| None, arr),
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-PARAMS",
            "params must be an object for named parameters (:name) or an array for positional parameters (?)",
            None,
        )),
    }
}

/// Bind a typed [`Value`] `params` into a query string.
///
/// Same contract as [`bind_str_with_params`], minus the JSON layer.
pub fn bind_str_with_values(
    query: &str,
    params: &Value,
    truncate_vectors: bool,
) -> Result<String, QqlError> {
    match params {
        Value::Dict(obj) => {
            let map = flatten_value_object(obj)?;
            if truncate_vectors {
                bind_named_readable(query, |k| map.get(k).cloned(), 2)
            } else {
                bind_named(query, |k| map.get(k).cloned())
            }
        }
        Value::List(arr) => {
            if truncate_vectors {
                bind_positional_readable(query, arr, 2)
            } else {
                bind_positional(query, arr)
            }
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-PARAMS",
            "params must be an object for named parameters (:name) or an array for positional parameters (?)",
            None,
        )),
    }
}

/// How a typed [`Value`] `params` argument applies to a batch of statements.
///
/// Same contract as [`plan_statement_params`]: a non-empty list whose entries
/// are **all** dicts or lists is statement-scoped (entry *i* binds statement
/// *i*, length must match — `QQL-BIND-BATCH-LENGTH` otherwise); every other
/// shape applies identically to every statement. `F32Array` counts as a
/// scalar here (a vector value, never a per-statement container).
#[derive(Debug)]
pub enum ValueParamPlan<'a> {
    /// The same params value binds every statement.
    Shared(&'a Value),
    /// One params container per statement; length matches the statement count.
    Scoped(&'a [Value]),
}

/// Decide how typed `params` applies to `stmt_count` statements.
pub fn plan_value_params(
    params: &Value,
    stmt_count: usize,
) -> Result<ValueParamPlan<'_>, QqlError> {
    if let Value::List(arr) = params
        && !arr.is_empty()
        && arr
            .iter()
            .all(|p| matches!(p, Value::Dict(_) | Value::List(_)))
    {
        if arr.len() == stmt_count {
            return Ok(ValueParamPlan::Scoped(arr));
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
    Ok(ValueParamPlan::Shared(params))
}

/// Resolve the params container for statement `index` under `plan`.
pub fn param_value_for<'a>(plan: &'a ValueParamPlan<'a>, index: usize) -> &'a Value {
    match plan {
        ValueParamPlan::Shared(params) => params,
        ValueParamPlan::Scoped(list) => &list[index],
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
    fn flatten_rejects_duplicate_colliding_keys() {
        let err = flatten_object(
            json!({"loc.lat": 1, "loc": {"lat": 2}})
                .as_object()
                .unwrap(),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BIND-DUPLICATE-PARAM");
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
    fn matrix_param_binds_as_multidense_and_matches_implicit_spelling() {
        // The explicit `QUERY VECTOR :x` spelling must parse to the same
        // parameter node as the implicit `QUERY :x USING` form, and a matrix
        // param must bind as a multi-vector on both (ColBERT prepared
        // statements).
        let mut explicit = Parser::parse("QUERY VECTOR :q FROM docs USING dense").unwrap();
        let mut implicit = Parser::parse("QUERY :q FROM docs USING dense").unwrap();
        let params = json!({"q": [[0.1, 0.2], [0.3, 0.4]]});
        bind_stmt_with_params(&mut explicit, &params).unwrap();
        bind_stmt_with_params(&mut implicit, &params).unwrap();
        let rendered = crate::fmt::format_stmt(&explicit);
        assert_eq!(rendered, crate::fmt::format_stmt(&implicit));
        assert!(
            rendered.contains("[[0.1, 0.2], [0.3, 0.4]]"),
            "matrix must render as a nested vector literal, got: {rendered}"
        );
    }

    #[test]
    fn matrix_param_rejects_non_numeric_rows() {
        let mut stmt = Parser::parse("QUERY VECTOR :q FROM docs USING dense").unwrap();
        let err = bind_stmt_with_params(&mut stmt, &json!({"q": [[0.1], ["x"]]})).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-TYPE-MISMATCH");
    }

    #[test]
    fn null_param_fails_closed_with_a_clear_code() {
        // A literal None/null parameter used to render as `null` text and
        // fail downstream with a misleading "query input requires …" error.
        let mut stmt = Parser::parse("QUERY :q FROM docs USING dense").unwrap();
        let err = bind_stmt_with_params(&mut stmt, &json!({"q": null})).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-NULL-PARAM");

        let mut stmt = Parser::parse("QUERY ? FROM docs USING dense").unwrap();
        let err = bind_stmt_with_params(&mut stmt, &json!([null])).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-NULL-PARAM");

        let err =
            bind_str_with_params("QUERY :q FROM docs USING dense", &json!({"q": null}), false)
                .unwrap_err();
        assert_eq!(err.code, "QQL-BIND-NULL-PARAM");

        // Nulls *inside* containers stay legal (payload fields are JSON).
        let mut stmt = Parser::parse("QUERY VECTOR :v FROM docs USING dense").unwrap();
        bind_stmt_with_params(&mut stmt, &json!({"v": [0.1]})).unwrap();
    }

    #[test]
    fn formula_param_binds_datetime_on_the_ast_path() {
        // N3: `TARGET = :now` used to store the bare identifier (indistinguishable
        // from a DEFAULTS key), so the prepared path shipped an unresolved
        // variable and the server answered "Expected number value for
        // judgment_date". The parser now preserves the ':' prefix, so binding
        // an ISO string produces the same Datetime node the inline form has.
        let mut stmt = Parser::parse(
            "QUERY FORMULA GAUSS_DECAY(DATETIME_KEY('judgment_date'), TARGET = :now) FROM docs",
        )
        .unwrap();
        bind_stmt_with_params(&mut stmt, &json!({"now": "2024-01-01T00:00:00Z"})).unwrap();
        let rendered = crate::fmt::format_stmt(&stmt);
        assert!(
            rendered.contains("TARGET = DATETIME('2024-01-01T00:00:00Z')"),
            "bound target must render as an inline datetime, got: {rendered}"
        );
        assert!(
            !rendered.contains(":now"),
            "no unresolved variable may remain: {rendered}"
        );
    }

    #[test]
    fn formula_defaults_keys_stay_variables() {
        // Bare identifiers (no colon) are DEFAULTS-bound keys, not params —
        // they must survive binding untouched.
        let mut stmt = Parser::parse(
            "QUERY FORMULA GAUSS_DECAY(rank, TARGET = 100.0, SCALE = 10.0) DEFAULTS (rank = 0.0) FROM docs",
        )
        .unwrap();
        bind_stmt_with_params(&mut stmt, &json!({"rank": 5})).unwrap();
        let rendered = crate::fmt::format_stmt(&stmt);
        assert!(
            rendered.contains("GAUSS_DECAY(rank"),
            "DEFAULTS key must stay: {rendered}"
        );
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
            "QUERY [0.1] FROM docs WHERE x = 1 LIMIT 5"
        );
        assert_eq!(
            crate::fmt::format_stmt(&stmts[1]),
            "QUERY [0.2] FROM docs WHERE y = 'k'"
        );
    }

    #[test]
    fn bind_stmt_with_values_named_and_dotted() {
        use crate::ast::Value;
        let mut stmt =
            Parser::parse("QUERY [0.1] FROM docs WHERE x = :x AND y = :loc.lat").unwrap();
        let params = Value::Dict(alloc::vec![
            ("x".into(), Value::Int(1)),
            (
                "loc".into(),
                Value::Dict(alloc::vec![("lat".into(), Value::Int(2))])
            ),
        ]);
        bind_stmt_with_values(&mut stmt, &params).unwrap();
        assert_eq!(
            crate::fmt::format_stmt(&stmt),
            "QUERY [0.1] FROM docs WHERE x = 1 AND y = 2"
        );
    }

    #[test]
    fn bind_stmt_with_values_positional_and_invalid_shape() {
        use crate::ast::Value;
        let mut stmt = Parser::parse("QUERY ? FROM docs USING dense LIMIT ?").unwrap();
        let params = Value::List(alloc::vec![
            Value::List(alloc::vec![Value::Float(0.5)]),
            Value::Int(7),
        ]);
        bind_stmt_with_values(&mut stmt, &params).unwrap();
        assert_eq!(
            crate::fmt::format_stmt(&stmt),
            "QUERY [0.5] FROM docs USING dense LIMIT 7"
        );

        let mut stmt = Parser::parse("QUERY [0.1] FROM docs").unwrap();
        let err = bind_stmt_with_values(&mut stmt, &Value::Int(1)).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-INVALID-PARAMS");
    }

    #[test]
    fn bind_stmt_with_values_scoped_batch() {
        use crate::ast::Value;
        let mut stmts = Parser::parse_all(
            "QUERY [0.1] FROM docs WHERE x = :x; QUERY [0.2] FROM docs WHERE y = :y;",
        )
        .unwrap();
        let params = Value::List(alloc::vec![
            Value::Dict(alloc::vec![("x".into(), Value::Int(1))]),
            Value::Dict(alloc::vec![("y".into(), Value::Int(2))]),
        ]);
        let plan = plan_value_params(&params, stmts.len()).unwrap();
        for (i, stmt) in stmts.iter_mut().enumerate() {
            bind_stmt_with_values(stmt, param_value_for(&plan, i)).unwrap();
        }
        assert_eq!(
            crate::fmt::format_stmt(&stmts[1]),
            "QUERY [0.2] FROM docs WHERE y = 2"
        );
        let err = plan_value_params(&params, 3).unwrap_err();
        assert_eq!(err.code, "QQL-BIND-BATCH-LENGTH");
    }

    #[test]
    fn bind_stmt_with_values_matches_json_path() {
        // Same logical params through both contracts must bind identically.
        use crate::ast::Value;
        let params_json = serde_json::json!({"x": 1, "v": [0.1, 0.2]});
        let params_value = Value::Dict(alloc::vec![
            ("x".into(), Value::Int(1)),
            (
                "v".into(),
                Value::List(alloc::vec![Value::Float(0.1), Value::Float(0.2)]),
            ),
        ]);
        let mut a = Parser::parse("QUERY :v FROM docs USING dense WHERE x = :x").unwrap();
        let mut b = a.clone();
        bind_stmt_with_params(&mut a, &params_json).unwrap();
        bind_stmt_with_values(&mut b, &params_value).unwrap();
        assert_eq!(crate::fmt::format_stmt(&a), crate::fmt::format_stmt(&b));
    }

    #[test]
    fn bind_stmt_with_values_f32array_vector() {
        // Typed-array fast path: F32Array binds without per-element boxing.
        use crate::ast::Value;
        let mut stmt = Parser::parse("QUERY :v FROM docs USING dense").unwrap();
        let params = Value::Dict(alloc::vec![(
            "v".into(),
            Value::F32Array(alloc::vec![0.1, 0.2])
        )]);
        bind_stmt_with_values(&mut stmt, &params).unwrap();
        assert_eq!(
            crate::fmt::format_stmt(&stmt),
            "QUERY [0.1, 0.2] FROM docs USING dense"
        );
    }

    #[test]
    fn bind_str_with_values_renders_vectors() {
        use crate::ast::Value;
        let params = Value::Dict(alloc::vec![("v".into(), Value::F32Array(alloc::vec![0.5]))]);
        let bound = bind_str_with_values("QUERY :v FROM docs USING dense", &params, false).unwrap();
        assert_eq!(bound, "QUERY [0.5] FROM docs USING dense");
    }

    #[test]
    fn flat_multivector_dict_binds_like_nested() {
        // {"data": [...], "dim": N} spells the same MultiDense as nested lists.
        let mut flat = Parser::parse("QUERY VECTOR :q FROM docs USING dense").unwrap();
        bind_stmt_with_params(
            &mut flat,
            &serde_json::json!({"q": {"data": [0.1, 0.2, 0.3, 0.4], "dim": 2}}),
        )
        .unwrap();
        let mut nested = Parser::parse("QUERY VECTOR :q FROM docs USING dense").unwrap();
        bind_stmt_with_params(
            &mut nested,
            &serde_json::json!({"q": [[0.1, 0.2], [0.3, 0.4]]}),
        )
        .unwrap();
        assert_eq!(
            crate::fmt::format_stmt(&flat),
            crate::fmt::format_stmt(&nested)
        );
    }

    #[test]
    fn flat_multivector_dict_rejects_bad_shapes() {
        for params in [
            serde_json::json!({"q": {"data": [0.1, 0.2]}}),
            serde_json::json!({"q": {"data": [0.1, 0.2, 0.3], "dim": 2}}),
            serde_json::json!({"q": {"data": [0.1], "dim": 0}}),
            serde_json::json!({"q": {"data": [0.1], "dim": 2, "indices": [0]}}),
        ] {
            let mut stmt = Parser::parse("QUERY VECTOR :q FROM docs USING dense").unwrap();
            let err = bind_stmt_with_params(&mut stmt, &params).unwrap_err();
            assert_eq!(err.code, "QQL-VALIDATION-VECTOR", "params: {params}");
        }
    }

    #[test]
    fn sparse_values_accept_f32array() {
        use crate::ast::Value;
        let mut stmt = Parser::parse("QUERY VECTOR :q FROM docs USING sparse").unwrap();
        let params = Value::Dict(alloc::vec![(
            "q".into(),
            Value::Dict(alloc::vec![
                (
                    "indices".into(),
                    Value::List(alloc::vec![Value::Int(1), Value::Int(5)])
                ),
                ("values".into(), Value::F32Array(alloc::vec![0.5, 0.8])),
            ]),
        )]);
        bind_stmt_with_values(&mut stmt, &params).unwrap();
        let rendered = crate::fmt::format_stmt(&stmt);
        assert!(rendered.contains("indices"), "got: {rendered}");
    }
}
