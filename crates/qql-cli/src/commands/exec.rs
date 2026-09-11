//! `qql exec` / `execute` / `explain` / `connect`.

use super::runtime::{executor, explain_query_bound};
use crate::output;
use crate::script;

pub async fn handle_exec(
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

pub async fn handle_execute_file(
    url: &str,
    use_edge: bool,
    path: &str,
    stop_on_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let statements = script::read_script(path).map_err(|e| format!("{}", e))?;
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
        match executor.execute(statement, on_err).await {
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
        "Executed script {} ({} succeeded, {} failed)",
        path, ok_count, fail_count
    );

    let resp = output::ScriptResponse {
        ok: fail_count == 0,
        command: "execute".to_string(),
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
