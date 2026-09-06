//! Shared client dispatch: input normalization, parameter planning, and the
//! blocking/async run loops used by `pyqql` and `pyqql-edge` clients.

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList};
use qql_core::ast;
use qql_core::error::QqlError;
use qql_core::params_json::{
    ParamPlan, bind_stmt_with_params, bind_str_with_params, param_for, plan_statement_params,
};
use qql_core::parser::Parser;

use crate::{PyStmt, py_to_json, qql_py_syntax_error, qql_py_value_error};

/// Executor error mode: stop the batch on the first failure, or continue.
pub type OnError = qql::executor::OnError;

/// Normalized execute input: one statement, a script string, or a batch.
pub enum Input {
    /// A single (possibly multi-statement) query string.
    String(String),
    /// A single pre-parsed statement.
    Stmt(ast::Stmt),
    /// A batch of query strings.
    StrList(Vec<String>),
    /// A batch of pre-parsed statements.
    StmtList(Vec<ast::Stmt>),
}

/// Parse the `on_error` option (`"stop"` default, `"continue"`).
pub fn parse_on_error(s: &str) -> PyResult<OnError> {
    match s {
        "stop" => Ok(OnError::Stop),
        "continue" => Ok(OnError::Continue),
        _ => Err(PyValueError::new_err(
            "on_error must be 'stop' or 'continue'",
        )),
    }
}

/// Normalize `query` (str | Stmt | list) plus `params` into an [`Input`],
/// applying the shared statement-scoped batch contract.
pub fn prepare_input(
    query: &Bound<'_, PyAny>,
    params: Option<&Bound<'_, PyAny>>,
) -> PyResult<Input> {
    let params_opt = params.filter(|p| !p.is_none());

    if let Ok(list) = query.cast::<PyList>() {
        if list.is_empty() {
            return Ok(Input::StrList(Vec::new()));
        }
        // Convert params once; the shared planner enforces the scoped
        // length contract (QQL-BIND-BATCH-LENGTH on mismatch).
        let json_params = match params_opt {
            Some(p) => Some(py_to_json(p)?),
            None => None,
        };
        let plan = match &json_params {
            Some(p) => Some(plan_statement_params(p, list.len()).map_err(qql_py_value_error)?),
            None => None,
        };

        let first = list.get_item(0)?;
        if first.extract::<PyRef<'_, PyStmt>>().is_ok() {
            let mut stmts = Vec::with_capacity(list.len());
            for (i, item) in list.iter().enumerate() {
                let py_stmt = item.extract::<PyRef<'_, PyStmt>>()?;
                let mut s = py_stmt.inner.clone();
                if let Some(plan) = &plan {
                    bind_stmt_with_params(&mut s, param_for(plan, i))
                        .map_err(qql_py_value_error)?;
                }
                stmts.push(s);
            }
            return Ok(Input::StmtList(stmts));
        }

        let mut strs = Vec::with_capacity(list.len());
        for (i, item) in list.iter().enumerate() {
            let s_str = item
                .extract::<String>()
                .map_err(|_| PyTypeError::new_err("list items must be strings or Stmt objects"))?;
            let bound = match &plan {
                Some(plan) => bind_str_with_params(&s_str, param_for(plan, i), false)
                    .map_err(qql_py_value_error)?,
                None => s_str,
            };
            strs.push(bound);
        }
        return Ok(Input::StrList(strs));
    }

    if let Ok(py_stmt) = query.extract::<PyRef<'_, PyStmt>>() {
        let mut stmt = py_stmt.inner.clone();
        if let Some(p) = params_opt {
            let json_params = py_to_json(p)?;
            let plan = plan_statement_params(&json_params, 1).map_err(qql_py_value_error)?;
            bind_stmt_with_params(&mut stmt, param_for(&plan, 0)).map_err(qql_py_value_error)?;
        }
        return Ok(Input::Stmt(stmt));
    }

    if let Ok(s) = query.extract::<String>() {
        if let Some(p) = params_opt {
            let json_params = py_to_json(p)?;
            // A params list of containers is a scoped candidate for scripts:
            // parse once to count statements, then plan.
            let scoped_candidate = matches!(&json_params, serde_json::Value::Array(arr)
                if !arr.is_empty() && arr.iter().all(|e| e.is_object() || e.is_array()));
            if scoped_candidate {
                let parsed = Parser::parse_all(&s).map_err(qql_py_syntax_error)?;
                let plan = plan_statement_params(&json_params, parsed.len())
                    .map_err(qql_py_value_error)?;
                if let ParamPlan::Scoped(list) = &plan {
                    let mut bound_stmts = Vec::with_capacity(parsed.len());
                    for (i, mut stmt) in parsed.into_iter().enumerate() {
                        bind_stmt_with_params(&mut stmt, &list[i]).map_err(qql_py_value_error)?;
                        bound_stmts.push(stmt);
                    }
                    return Ok(Input::StmtList(bound_stmts));
                }
                // Unreachable for container arrays (they scope or err); fall
                // through to whole-string binding defensively.
            }
            let bound =
                bind_str_with_params(&s, &json_params, false).map_err(qql_py_value_error)?;
            return Ok(Input::String(bound));
        }
        return Ok(Input::String(s));
    }

    Err(PyTypeError::new_err(
        "query must be a str, Stmt, list[str], or list[Stmt]",
    ))
}

/// Run a normalized [`Input`] on the blocking Tokio runtime.
pub fn run_input(
    executor: &qql::executor::Executor,
    runtime: &tokio::runtime::Runtime,
    input: Input,
    on_error: OnError,
) -> PyResult<serde_json::Value> {
    let stop = matches!(on_error, OnError::Stop);
    let map_err = |e: QqlError| PyRuntimeError::new_err(e.to_string());
    match input {
        Input::String(s) => {
            let report = runtime
                .block_on(executor.execute(&s, on_error))
                .map_err(map_err)?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::Stmt(s) => {
            let results = runtime
                .block_on(executor.execute_batch_nodes(vec![s], stop))
                .map_err(map_err)?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StrList(strs) => {
            let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
            let report = runtime
                .block_on(executor.execute_batch(&refs, on_error))
                .map_err(map_err)?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StmtList(stmts) => {
            let results = runtime
                .block_on(executor.execute_batch_nodes(stmts, stop))
                .map_err(map_err)?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
    }
}

/// Run a normalized [`Input`] on an existing async context.
pub async fn run_async(
    executor: &qql::executor::Executor,
    input: Input,
    on_error: OnError,
) -> Result<serde_json::Value, QqlError> {
    let stop = matches!(on_error, OnError::Stop);
    match input {
        Input::String(s) => {
            let report = executor.execute(&s, on_error).await?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::Stmt(s) => {
            let results = executor.execute_batch_nodes(vec![s], stop).await?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StrList(strs) => {
            let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
            let report = executor.execute_batch(&refs, on_error).await?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StmtList(stmts) => {
            let results = executor.execute_batch_nodes(stmts, stop).await?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
    }
}

/// Wrap a serialized report dict in the host module's Python-level
/// `ExecutionReport` class (typed accessors), falling back to the plain dict
/// when the import fails (e.g. during interpreter teardown).
pub fn wrap_execution_report<'py>(
    py: Python<'py>,
    dict: Bound<'py, PyAny>,
    module_name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    if let Ok(module) = py.import(module_name)
        && let Ok(report_cls) = module.getattr("ExecutionReport")
        && let Ok(report) = report_cls.call1((&dict,))
    {
        return Ok(report);
    }
    Ok(dict)
}
