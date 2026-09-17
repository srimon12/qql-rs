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
    check_embedder_availability(&executor, query)?;
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
    // Single-query output keeps its table shape (no ScriptResponse print),
    // but shares the nonzero-on-failure contract so a future Continue mode
    // here cannot silently exit 0.
    ensure_run_ok(report.failed, "report above")
}

/// Shared exit contract for every `qql run` path (script file + single
/// query): a run with any failed statement exits nonzero. The `Err` only
/// points at the report already printed on stdout; it never duplicates the
/// counts. `report_ref` names where that report went.
fn ensure_run_ok(failed: usize, report_ref: &str) -> Result<(), Box<dyn std::error::Error>> {
    if failed > 0 {
        return Err(format!("run failed; see {report_ref}").into());
    }
    Ok(())
}

/// The ONE `qql run` script outcome: print the ScriptResponse JSON shape on
/// stdout, then enforce the shared exit contract. Both Stop and Continue
/// modes end here so consumers parse one shape.
fn finish_script(
    path: &str,
    succeeded: usize,
    failed: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let ok = failed == 0;
    let resp = output::ScriptResponse {
        ok,
        command: "run".to_string(),
        path: path.to_string(),
        succeeded,
        failed,
        message: format!("Ran script {path} ({succeeded} succeeded, {failed} failed)"),
    };
    println!("{}", serde_json::to_string_pretty(&resp)?);
    ensure_run_ok(failed, "JSON report on stdout")
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
    for statement in &statements {
        check_embedder_availability(&executor, statement)?;
    }
    let mut ok_count = 0;
    let mut fail_count = 0;

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
                    break;
                }
            }
        }
    }
    executor.close().await?;
    finish_script(path, ok_count, fail_count)
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

fn check_embedder_availability(
    executor: &qql::executor::Executor,
    query: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if executor.embedder().is_some() {
        return Ok(());
    }
    // Check if any parsed statement requires an embedder
    if let Ok(stmts) = qql_core::parser::Parser::parse_all(query) {
        for stmt in &stmts {
            if statement_requires_embedder(stmt) {
                return Err(qql_core::error::QqlError::execution(
                    "QQL-EMBEDDING-UNAVAILABLE",
                    "No embedder configured for dense text/image queries.\n\
                    → Pass precomputed vectors via: QUERY :vec ... with --params-file <file.json>\n\
                    → Or configure a remote embedding server: qql config set embed_url http://localhost:11434/v1\n\
                    → Or install with local zero-server ONNX embeddings: cargo install qql-cli --locked --features fastembed\n\
                    → Or install full package: cargo install qql-cli --locked --features full",
                    None,
                )
                .into());
            }
        }
    }
    Ok(())
}

fn statement_requires_embedder(stmt: &qql_core::ast::Stmt) -> bool {
    match stmt {
        qql_core::ast::Stmt::Query(q) => query_stmt_requires_embedder(q),
        qql_core::ast::Stmt::Upsert(u) => u.embedding.is_some(),
        _ => false,
    }
}

fn query_stmt_requires_embedder(q: &qql_core::ast::QueryStmt) -> bool {
    for cte in &q.ctes {
        if query_stmt_requires_embedder(&cte.query) {
            return true;
        }
    }
    query_expr_requires_embedder(&q.expression)
}

fn query_expr_requires_embedder(expr: &qql_core::ast::QueryExpr) -> bool {
    use qql_core::ast::{PrefetchSource, QueryExpr};
    let prefetch_requires = |prefetches: &[qql_core::ast::Prefetch]| {
        prefetches.iter().any(|p| match &p.source {
            PrefetchSource::Query(sub) => query_stmt_requires_embedder(sub),
            _ => false,
        })
    };
    match expr {
        QueryExpr::Nearest {
            input, prefetch, ..
        } => is_embeddable_input(input) || prefetch_requires(prefetch),
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            positive.iter().any(is_embeddable_input)
                || negative.iter().any(is_embeddable_input)
                || prefetch_requires(prefetch)
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            pairs
                .iter()
                .any(|p| is_embeddable_input(&p.positive) || is_embeddable_input(&p.negative))
                || prefetch_requires(prefetch)
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            is_embeddable_input(target)
                || context
                    .iter()
                    .any(|p| is_embeddable_input(&p.positive) || is_embeddable_input(&p.negative))
                || prefetch_requires(prefetch)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            is_embeddable_input(target)
                || feedback.iter().any(|f| is_embeddable_input(&f.example))
                || prefetch_requires(prefetch)
        }
        QueryExpr::Hybrid { .. } => true,
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            prefetch_requires(prefetch)
        }
        _ => false,
    }
}

fn is_embeddable_input(input: &qql_core::ast::QueryInput) -> bool {
    matches!(
        input,
        qql_core::ast::QueryInput::Text { .. } | qql_core::ast::QueryInput::Image { .. }
    )
}
