//! Shared execution dispatch for `nqql` / `nqql-edge`.
//!
//! One implementation of the `execute(query, options)` contract — string
//! scripts, arrays of strings/Stmts, single Stmts, named/positional/statement-
//! scoped parameters — so the two Node SDKs cannot diverge. The rule for
//! parameter dispatch is [`qql_core::params_json::plan_value_params`].

use napi::bindgen_prelude::{FromNapiValue, JsObjectValue as _, Object, Unknown};
use qql_core::ast;
use qql_core::error::QqlError;
use qql_core::params_json::{
    ValueParamPlan, bind_stmt_with_values, bind_str_with_values, param_value_for, plan_value_params,
};
use qql_core::parser::Parser;
use std::future::Future;

use crate::{jsparams, to_napi_err};

/// Parse the `onError` option (`"stop"` default, `"continue"`).
pub fn on_error_from(options: Option<&serde_json::Value>) -> qql::executor::OnError {
    options
        .and_then(|o| o.get("onError"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "continue" => qql::executor::OnError::Continue,
            _ => qql::executor::OnError::Stop,
        })
        .unwrap_or(qql::executor::OnError::Stop)
}

// ── Typed execute options (P0-1) ─────────────────────────────────
//
// napi's serde layer mangles `Float32Array` / `Float64Array` (and integer
// typed arrays) inside `options.params` into index-keyed objects — silently
// wrong bindings. This bag converts `params` through the typed
// [`jsparams::unknown_opt_to_value`] path instead, synchronously on the JS
// thread during argument conversion, so only owned `serde_json::Value` /
// [`ast::Value`] cross into the async future. The JS calling convention is
// unchanged: a single options object (or absent).

/// Execute options with host-typed `params` (typed arrays preserved).
///
/// `raw` is the whole options bag as JSON — client construction reads
/// url/apiKey/… from it (any mangled `params` inside is ignored for
/// binding). `params` is `None` for absent options or absent/`null` params,
/// mirroring the old `Option<serde_json::Value>` contract.
pub struct ExecOptionsInput {
    /// Whole options bag as JSON — client construction reads url/apiKey/…
    /// from it (any mangled `params` inside is ignored for binding).
    pub raw: serde_json::Value,
    /// Params converted through the typed path (`None` for absent options
    /// or absent/`null` params).
    pub params: Option<ast::Value>,
}

impl FromNapiValue for ExecOptionsInput {
    unsafe fn from_napi_value(
        env: napi::sys::napi_env,
        napi_val: napi::sys::napi_value,
    ) -> napi::Result<Self> {
        // SAFETY: napi invokes argument conversion synchronously on the JS
        // thread with valid handles (trait contract). Everything below uses
        // the safe `Unknown` / `Object` / serde wrappers; only owned values
        // escape into the async future, so it stays `Send + 'static`.
        let raw = unsafe { serde_json::Value::from_napi_value(env, napi_val) }
            .map_err(|e| napi::Error::from_reason(format!("invalid options object: {e}")))?;
        let unknown = unsafe { Unknown::from_napi_value(env, napi_val) }?;
        let params = match unknown.get_type()? {
            napi::ValueType::Object => {
                let param_value: Option<Unknown> = Object::from_unknown(unknown)
                    .ok()
                    .and_then(|obj| obj.get_named_property("params").ok());
                match param_value {
                    Some(p) => jsparams::unknown_opt_to_value(p).map_err(to_napi_err)?,
                    None => None,
                }
            }
            _ => None,
        };
        Ok(Self { raw, params })
    }
}

/// Split an [`ExecOptionsInput`] (or its absence) into dispatch inputs.
pub fn typed_dispatch_inputs(
    options: Option<&ExecOptionsInput>,
) -> (qql::executor::OnError, Option<&ast::Value>) {
    match options {
        Some(o) => (on_error_from(Some(&o.raw)), o.params.as_ref()),
        None => (qql::executor::OnError::Stop, None),
    }
}

/// Parse the `batchSize` option (default 100). Values `< 1` fail closed with
/// the same code as the Rust `Executor::upsert_many` core, so JS callers get
/// the identical error without any I/O.
pub fn batch_size_from(options: Option<&serde_json::Value>) -> Result<usize, QqlError> {
    match options.and_then(|o| o.get("batchSize")) {
        None => Ok(100),
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 => Ok(n as usize),
            _ => Err(QqlError::validation(
                "QQL-VALIDATION-UPSERT-BATCH",
                "upsertMany batchSize must be >= 1",
                None,
            )),
        },
    }
}

/// Bulk ingest dispatch shared by both Node SDKs: `rows` must be the
/// `Value::List` of point dicts produced by [`jsparams::unknown_to_value`]
/// (typed arrays preserved — never the serde layer, which would mangle
/// `Float32Array` into index-keyed objects). Non-list input fails closed
/// with the same code as whole-point `Stmt.bind`.
pub async fn upsert_many_dispatch(
    executor: &qql::executor::Executor,
    collection: String,
    rows: ast::Value,
    batch_size: usize,
    on_error: qql::executor::OnError,
) -> Result<qql::executor::ExecutionReport, QqlError> {
    let rows = match rows {
        ast::Value::List(rows) => rows,
        _ => {
            return Err(QqlError::validation(
                "QQL-BIND-TYPE-MISMATCH",
                "upsertMany rows must be an array of point objects ({id, vector, …payload})",
                None,
            ));
        }
    };
    executor
        .upsert_many(&collection, rows, batch_size, on_error)
        .await
}

/// Close a one-shot executor after `run` completes, so a failed dispatch
/// cannot leak the underlying connection (for edge: on-disk shards and the
/// loaded model). Mirrors the Python wrapper's `try/finally: close()`
/// contract: on the error path the dispatch error wins and the close is
/// best-effort; after a *successful* dispatch a close failure is surfaced
/// (an unflushed edge write must not be silently swallowed).
pub async fn run_then_close<T, E>(executor: &qql::executor::Executor, run: E) -> Result<T, QqlError>
where
    E: Future<Output = Result<T, QqlError>>,
{
    match run.await {
        Ok(value) => {
            executor.close().await?;
            Ok(value)
        }
        Err(e) => {
            let _ = executor.close().await;
            Err(e)
        }
    }
}

/// True when typed `params` is a non-empty list whose entries are all dicts
/// or lists — a statement-scoped candidate under the shared batch contract.
///
/// `F32Array` counts as a scalar here (a vector value, never a per-statement
/// container).
fn scoped_value_candidate(params: Option<&ast::Value>) -> bool {
    matches!(params, Some(ast::Value::List(items))
        if !items.is_empty()
            && items
                .iter()
                .all(|p| matches!(p, ast::Value::Dict(_) | ast::Value::List(_))))
}

/// Analyze a single QQL query string or Stmt with host-typed `params`:
/// static plan plus measured execution (per-phase client timings, server
/// time, hardware/inference usage). Batches fail closed
/// (`QQL-VALIDATION-ANALYZE-BATCH`) instead of silently analyzing one
/// entry — analyze each entry separately.
///
/// `params` already went through [`jsparams::unknown_opt_to_value`], so
/// typed arrays bind packed instead of arriving as index-keyed objects.
pub async fn explain_analyze_dispatch_typed(
    executor: &qql::executor::Executor,
    query: serde_json::Value,
    on_error: qql::executor::OnError,
    params: Option<&ast::Value>,
) -> Result<qql::executor::AnalyzeReport, QqlError> {
    match &query {
        serde_json::Value::String(s) => {
            let bound = match params {
                Some(p) => bind_str_with_values(s, p, false)?,
                None => s.clone(),
            };
            executor.explain_analyze(&bound, on_error).await
        }
        serde_json::Value::Array(_) => Err(QqlError::validation(
            "QQL-VALIDATION-ANALYZE-BATCH",
            "explainAnalyze accepts a single statement (string or Stmt), not a batch; analyze each entry separately",
            None,
        )),
        _ => {
            let mut s: ast::Stmt = serde_json::from_value(query).map_err(|e| {
                QqlError::validation("QQL-BATCH-INVARIANT", format!("invalid Stmt: {e}"), None)
            })?;
            if let Some(p) = params {
                bind_stmt_with_values(&mut s, p)?;
            }
            executor.explain_analyze_node(s, on_error).await
        }
    }
}

/// Execute a QQL query string, a Stmt, or an array of either against
/// `executor`, binding host-typed `params` per the shared batch contract.
///
/// Multi-statement strings (semicolons) and arrays are auto-batched. Returns
/// the transport-neutral `qql::executor::ExecutionReport`; the SDK crates
/// serialize it for their JS wrapper.
///
/// `params` already went through [`jsparams::unknown_opt_to_value`], so
/// typed arrays bind packed instead of arriving as index-keyed objects.
/// Requested statements keep JSON form — Stmt JSON from `toObject()` is
/// always JSON-safe; only `params` needed the typed path.
pub async fn execute_dispatch_typed(
    executor: &qql::executor::Executor,
    query: serde_json::Value,
    on_error: qql::executor::OnError,
    params: Option<&ast::Value>,
) -> Result<qql::executor::ExecutionReport, QqlError> {
    let stop = matches!(on_error, qql::executor::OnError::Stop);

    match &query {
        serde_json::Value::String(s) => {
            // A scoped params list for a string input: parse once to count
            // the script's statements, then let the shared planner enforce
            // the exact length contract.
            if scoped_value_candidate(params) {
                let p = params.unwrap_or(&ast::Value::Null);
                let mut stmts = Parser::parse_all(s)?;
                let plan = plan_value_params(p, stmts.len())?;
                if let ValueParamPlan::Scoped(list) = &plan {
                    for (i, stmt) in stmts.iter_mut().enumerate() {
                        bind_stmt_with_values(stmt, &list[i])?;
                    }
                    let results = executor.execute_batch_nodes(stmts, stop).await?;
                    return Ok(qql::executor::ExecutionReport::from_results(results));
                }
                // Container lists always scope or err, so the Shared arm is
                // unreachable; fall through to whole-string binding for
                // robustness against future planner changes.
            }
            let bound = match params {
                Some(p) => bind_str_with_values(s, p, false)?,
                None => s.clone(),
            };
            executor.execute(&bound, on_error).await
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                // Fail closed like the executor does for an empty string —
                // an empty array must not return a silently-empty ok report.
                return Err(QqlError::validation(
                    "QQL-VALIDATION-EMPTY-SCRIPT",
                    "no statements to execute; the query is empty or contains only whitespace",
                    None,
                ));
            }
            let plan = match params {
                Some(p) => Some(plan_value_params(p, arr.len())?),
                None => None,
            };
            if arr[0].is_string() {
                let mut bound_strs = Vec::with_capacity(arr.len());
                for (i, v) in arr.iter().enumerate() {
                    let s = v.as_str().ok_or_else(|| {
                        QqlError::validation(
                            "QQL-BATCH-INVARIANT",
                            "batch items must be strings",
                            None,
                        )
                    })?;
                    let bound = match &plan {
                        Some(plan) => bind_str_with_values(s, param_value_for(plan, i), false)?,
                        None => s.to_string(),
                    };
                    bound_strs.push(bound);
                }
                let refs: Vec<&str> = bound_strs.iter().map(String::as_str).collect();
                executor.execute_batch(&refs, on_error).await
            } else {
                let mut stmts = Vec::with_capacity(arr.len());
                for (i, v) in arr.iter().enumerate() {
                    let mut s: ast::Stmt = serde_json::from_value(v.clone()).map_err(|e| {
                        QqlError::validation(
                            "QQL-BATCH-INVARIANT",
                            format!("invalid Stmt: {e}"),
                            None,
                        )
                    })?;
                    if let Some(plan) = &plan {
                        bind_stmt_with_values(&mut s, param_value_for(plan, i))?;
                    }
                    stmts.push(s);
                }
                let results = executor.execute_batch_nodes(stmts, stop).await?;
                Ok(qql::executor::ExecutionReport::from_results(results))
            }
        }
        _ => {
            let mut s: ast::Stmt = serde_json::from_value(query).map_err(|e| {
                QqlError::validation("QQL-BATCH-INVARIANT", format!("invalid Stmt: {e}"), None)
            })?;
            if let Some(p) = params {
                bind_stmt_with_values(&mut s, p)?;
            }
            let results = executor.execute_batch_nodes(vec![s], stop).await?;
            Ok(qql::executor::ExecutionReport::from_results(results))
        }
    }
}
