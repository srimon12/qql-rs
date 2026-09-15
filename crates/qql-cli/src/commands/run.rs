//! `qql run` / `explain` / `repl`.

use super::runtime::{executor, explain_query_bound};
use crate::output;
use crate::script;

pub async fn handle_run_smart(
    url: &str,
    use_edge: bool,
    query_or_file: &str,
    params: Option<&serde_json::Value>,
    stop_on_error: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let p = std::path::Path::new(query_or_file);
    let looks_like_path = query_or_file.ends_with(".qql")
        || ((query_or_file.contains('/') || query_or_file.contains('\\'))
            && !query_or_file.contains(char::is_whitespace));
    let is_file = p.is_file() || (!query_or_file.contains('\n') && looks_like_path);
    if is_file {
        handle_run_file(url, use_edge, query_or_file, params, stop_on_error).await
    } else {
        handle_run(url, use_edge, query_or_file, params, json, quiet).await
    }
}

pub async fn handle_run(
    url: &str,
    use_edge: bool,
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let result = match params {
        None => executor.execute(query, qql::executor::OnError::Stop).await,
        Some(serde_json::Value::Object(obj)) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), qql_core::ast::Value::from_json(v.clone())?);
            }
            executor
                .execute_with_params(query, &map, qql::executor::OnError::Stop)
                .await
        }
        Some(serde_json::Value::Array(arr)) => {
            let items: Vec<qql_core::ast::Value> = arr
                .iter()
                .cloned()
                .map(qql_core::ast::Value::from_json)
                .collect::<Result<_, _>>()?;
            executor
                .execute_with_positional_params(query, &items, qql::executor::OnError::Stop)
                .await
        }
        Some(_) => {
            return Err("--params-file must contain a JSON object or array".into());
        }
    };
    executor.close().await?;
    let report = result?;
    if !quiet {
        crate::table::render_report(&report, json)?;
    }
    Ok(())
}

pub async fn handle_run_file(
    url: &str,
    use_edge: bool,
    path: &str,
    params: Option<&serde_json::Value>,
    stop_on_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let statements = script::read_script(path).map_err(|e| format!("{}", e))?;
    // Statement-scoped binding (same contract as the SDK `execute` batch
    // path): an array whose entries are all objects/arrays binds entry `i`
    // to statement `i` — a length mismatch fails closed here, before any
    // statement runs (`QQL-BIND-BATCH-LENGTH`). Any other shape binds
    // identically to every statement.
    let plan = params
        .map(|p| qql_core::params_json::plan_statement_params(p, statements.len()))
        .transpose()
        .map_err(|e| format!("{}", e))?;
    let executor = executor(url, use_edge)?;
    let mut ok_count = 0;
    let mut fail_count = 0;
    let mut fatal_error = None;

    for (index, statement) in statements.iter().enumerate() {
        let on_err = if stop_on_error {
            qql::executor::OnError::Stop
        } else {
            qql::executor::OnError::Continue
        };
        let step = match plan.as_ref() {
            None => executor.execute(statement, on_err).await,
            Some(plan) => match qql_core::params_json::param_for(plan, index) {
                serde_json::Value::Object(obj) => {
                    let mut map = std::collections::HashMap::new();
                    for (k, v) in obj {
                        map.insert(k.clone(), qql_core::ast::Value::from_json(v.clone())?);
                    }
                    executor.execute_with_params(statement, &map, on_err).await
                }
                serde_json::Value::Array(arr) => {
                    let items: Vec<qql_core::ast::Value> = arr
                        .iter()
                        .cloned()
                        .map(qql_core::ast::Value::from_json)
                        .collect::<Result<_, _>>()?;
                    executor
                        .execute_with_positional_params(statement, &items, on_err)
                        .await
                }
                _ => {
                    return Err("--params-file must contain a JSON object or array".into());
                }
            },
        };
        match step {
            Ok(report) => {
                ok_count += report.succeeded;
                fail_count += report.failed;
                for result in report.results.iter().filter(|result| !result.ok) {
                    output::print_error(&format!(
                        "statement {} ({}): {}",
                        index + 1,
                        result.operation,
                        result.message
                    ));
                }
            }
            Err(error) => {
                fail_count += 1;
                output::print_error(&format!("statement {}: {}", index + 1, error));
                if stop_on_error {
                    fatal_error = Some(format!("statement {} failed: {}", index + 1, error));
                    break;
                }
            }
        }
    }
    executor.close().await?;
    if let Some(error) = fatal_error {
        return Err(error.into());
    }

    let msg = format!(
        "Ran script {} ({} succeeded, {} failed)",
        path, ok_count, fail_count
    );

    let resp = output::ScriptResponse {
        ok: fail_count == 0,
        command: "run".to_string(),
        path: path.to_string(),
        succeeded: ok_count,
        failed: fail_count,
        message: msg.clone(),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

pub fn handle_explain(
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query_bound(query, params)?;
    if quiet {
        return Ok(());
    }
    if json {
        let resp = output::ExplainResponse {
            ok: true,
            query: query.to_string(),
            plan,
        };
        let s = serde_json::to_string_pretty(&resp)?;
        println!("{}", s);
    } else {
        println!("{}", plan);
    }
    Ok(())
}
