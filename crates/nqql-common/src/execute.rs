//! Shared execution dispatch for `nqql` / `nqql-edge`.
//!
//! One implementation of the `execute(query, options)` contract — string
//! scripts, arrays of strings/Stmts, single Stmts, named/positional/statement-
//! scoped parameters — so the two Node SDKs cannot diverge. The rule for
//! parameter dispatch is [`qql_core::params_json::plan_statement_params`].

use qql_core::ast;
use qql_core::error::QqlError;
use qql_core::params_json::{
    bind_stmt_with_params, bind_str_with_params, param_for, plan_statement_params,
};
use qql_core::parser::Parser;

/// Parse the `onError` option (`"stop"` default, `"continue"`).
fn on_error_from(options: Option<&serde_json::Value>) -> qql::executor::OnError {
    options
        .and_then(|o| o.get("onError"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "continue" => qql::executor::OnError::Continue,
            _ => qql::executor::OnError::Stop,
        })
        .unwrap_or(qql::executor::OnError::Stop)
}

/// True when `params` is a non-empty array whose entries are all objects or
/// arrays — a statement-scoped candidate under the shared batch contract.
fn scoped_candidate(params: Option<&serde_json::Value>) -> bool {
    matches!(params, Some(serde_json::Value::Array(arr))
        if !arr.is_empty() && arr.iter().all(|p| p.is_object() || p.is_array()))
}

/// Execute a QQL query string, a Stmt, or an array of either against
/// `executor`, binding `options.params` per the shared batch contract.
///
/// Multi-statement strings (semicolons) and arrays are auto-batched. Returns
/// the transport-neutral [`ExecutionReport`]; the SDK crates serialize it for
/// their JS wrapper.
pub async fn execute_dispatch(
    executor: &qql::executor::Executor,
    query: serde_json::Value,
    options: Option<&serde_json::Value>,
) -> Result<qql::executor::ExecutionReport, QqlError> {
    let on_error = on_error_from(options);
    let stop = matches!(on_error, qql::executor::OnError::Stop);
    let params = options.and_then(|o| o.get("params"));

    match &query {
        serde_json::Value::String(s) => {
            // A scoped params array for a string input: parse once to count
            // the script's statements, then let the shared planner enforce
            // the exact length contract.
            if scoped_candidate(params) {
                let p = params.unwrap_or(&serde_json::Value::Null);
                let mut stmts = Parser::parse_all(s)?;
                let plan = plan_statement_params(p, stmts.len())?;
                if let qql_core::params_json::ParamPlan::Scoped(list) = &plan {
                    for (i, stmt) in stmts.iter_mut().enumerate() {
                        bind_stmt_with_params(stmt, &list[i])?;
                    }
                    let results = executor.execute_batch_nodes(stmts, stop).await?;
                    return Ok(qql::executor::ExecutionReport::from_results(results));
                }
                // Container arrays always scope or err, so the Shared arm is
                // unreachable; fall through to whole-string binding for
                // robustness against future planner changes.
            }
            let bound = match params {
                Some(p) => bind_str_with_params(s, p, false)?,
                None => s.clone(),
            };
            executor.execute(&bound, on_error).await
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return Ok(qql::executor::ExecutionReport::empty());
            }
            let plan = match params {
                Some(p) => Some(plan_statement_params(p, arr.len())?),
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
                        Some(plan) => bind_str_with_params(s, param_for(plan, i), false)?,
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
                        bind_stmt_with_params(&mut s, param_for(plan, i))?;
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
                bind_stmt_with_params(&mut s, p)?;
            }
            let results = executor.execute_batch_nodes(vec![s], stop).await?;
            Ok(qql::executor::ExecutionReport::from_results(results))
        }
    }
}
