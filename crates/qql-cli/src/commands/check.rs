//! `qql check` — format, explain, embed probe, topology, doctor.

use super::convert::source_is_canonical;
use super::edge::{collect_indexing_states, count_label};
#[cfg(feature = "rest")]
use super::runtime::probe_embed_dim;
use super::runtime::{
    classify_backend_failure, dense_vector_sizes, executor, explain_query_bound,
    resolve_embed_settings,
};

/// Triage one statement through the documented debug loop:
/// format, offline explain, embed probe, topology, doctor.
pub async fn handle_check(
    url: &str,
    use_edge: bool,
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stages: Vec<serde_json::Value> = Vec::new();
    let mut failed = false;
    let mut push = |stage: &str, status: &str, code: Option<String>, message: String| {
        stages.push(serde_json::json!({
            "stage": stage,
            "status": status,
            "code": code,
            "message": message,
        }));
    };

    // Stage 1: format check of the input.
    match qql_core::fmt::format(query) {
        Ok(formatted) => {
            if source_is_canonical(query, &formatted) {
                push("format", "ok", None, "format: parses cleanly".to_string());
            } else {
                push(
                    "format",
                    "ok",
                    None,
                    "format: parses cleanly but is not canonical; fix: run `qql fmt`".to_string(),
                );
            }
        }
        Err(e) => {
            failed = true;
            push(
                "format",
                "fail",
                Some(e.code.to_string()),
                format!(
                    "[{}] format: {e}; fix: correct the syntax at the reported span",
                    e.code
                ),
            );
        }
    }

    // Stage 2: offline explain.
    let mut stmts: Option<Vec<qql_core::ast::Stmt>> = None;
    match explain_query_bound(query, params) {
        Ok(_) => {
            push(
                "explain",
                "ok",
                None,
                "explain: offline plan built without a backend".to_string(),
            );
            if let Ok(parsed) = parse_and_bind(query, params) {
                stmts = Some(parsed);
            }
        }
        Err(msg) => {
            failed = true;
            push(
                "explain",
                "fail",
                None,
                format!("explain: {msg}; fix: address the reported QQL-* code"),
            );
            if let Ok(parsed) = parse_and_bind(query, params) {
                stmts = Some(parsed);
            }
        }
    }

    // Stage 3: embed endpoint dim probe when the statement needs embeddings.
    let needs_embed = stmts
        .as_ref()
        .is_some_and(|s| statements_need_embeddings(s));
    // Updated only by the REST embed probe; grpc-only builds keep the default.
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut observed_dim: Option<usize> = None;
    if !needs_embed {
        push(
            "embed",
            "skip",
            None,
            "embed: literal vectors only, no embedder needed".to_string(),
        );
    } else if use_edge {
        push(
            "embed",
            "skip",
            None,
            "embed: edge backend provides local embeddings".to_string(),
        );
    } else {
        let (endpoint_opt, model, expected_dim, dim_source) = resolve_embed_settings();
        match endpoint_opt {
            None => {
                failed = true;
                push(
                    "embed",
                    "fail",
                    Some("QQL-EMBEDDING".to_string()),
                    "embed: [QQL-EMBEDDING] statement needs text embeddings but no EMBED_URL is set; fix: export EMBED_URL=http://localhost:11434/v1/embeddings and EMBED_DIM=384".to_string(),
                );
            }
            Some(endpoint) => {
                #[cfg(feature = "rest")]
                {
                    match probe_embed_dim(&endpoint, &model, expected_dim).await {
                        Ok(real) => {
                            observed_dim = Some(real);
                            if real == expected_dim {
                                push(
                                    "embed",
                                    "ok",
                                    None,
                                    format!(
                                        "embed: {endpoint} model={model} dim={real} (matches {dim_source})"
                                    ),
                                );
                            } else {
                                failed = true;
                                push(
                                    "embed",
                                    "fail",
                                    Some("QQL-EMBEDDING-DIM".to_string()),
                                    format!(
                                        "[QQL-EMBEDDING-DIM] embed: {endpoint} returned dim={real} but {dim_source}={expected_dim}; fix: set EMBED_DIM={real}"
                                    ),
                                );
                            }
                        }
                        Err(e) => {
                            failed = true;
                            push(
                                "embed",
                                "fail",
                                Some(e.code.to_string()),
                                format!(
                                    "[{}] embed: probe of {endpoint} failed: {e}; fix: run `ollama serve` and `ollama pull {model}`",
                                    e.code
                                ),
                            );
                        }
                    }
                }
                #[cfg(not(feature = "rest"))]
                {
                    let _ = (endpoint, expected_dim, model, dim_source);
                    push(
                        "embed",
                        "skip",
                        None,
                        "embed: HTTP probe needs the rest feature".to_string(),
                    );
                }
            }
        }
    }

    // Stages 4-5 need a backend. Build once and reuse for topology + doctor.
    let executor = executor(url, use_edge).ok();
    let mut backend_reachable = false;
    let mut backend_note = String::new();
    if let Some(exec) = executor.as_ref() {
        match exec
            .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
            .await
        {
            Ok(_) => backend_reachable = true,
            Err(e) => {
                let class = classify_backend_failure(&e.code, &e.message);
                backend_note = match class {
                    "unreachable" => format!(
                        "[{}] backend unreachable at {url}: {e}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                        e.code
                    ),
                    "auth" => format!(
                        "[QQL-BACKEND-AUTH] backend auth failed at {url}: {e}; fix: set QDRANT_API_KEY"
                    ),
                    _ => format!("[{}] backend check failed: {e}", e.code),
                };
            }
        }
    } else {
        backend_note =
            "backend: executor init failed; fix: reinstall with default features".to_string();
    }

    // Stage 4: USING and vector-name topology check against the live collection.
    if let Some(statements) = stmts.as_ref() {
        let collections = stmt_collections(statements);
        if collections.is_empty() {
            push(
                "topology",
                "skip",
                None,
                "topology: no target collection in this statement".to_string(),
            );
        } else if !backend_reachable {
            push(
                "topology",
                "skip",
                None,
                format!("topology: backend unreachable, cannot verify USING ({backend_note})"),
            );
        } else if let Some(exec) = executor.as_ref() {
            let mut ok_all = true;
            for collection in &collections {
                match exec.client().get_collection_info(collection).await {
                    Err(e) => {
                        ok_all = false;
                        failed = true;
                        push(
                            "topology",
                            "fail",
                            Some(e.code.to_string()),
                            format!(
                                "[{}] topology: collection '{collection}' lookup failed: {e}; fix: create it or check the name",
                                e.code
                            ),
                        );
                    }
                    Ok(info) => {
                        let dense: Vec<String> = info
                            .schema
                            .vectors
                            .iter()
                            .filter_map(|v| v.name.clone())
                            .collect();
                        let sparse: Vec<String> = info
                            .schema
                            .sparse_vectors
                            .iter()
                            .map(|v| v.name.clone())
                            .collect();
                        let unnamed = info.schema.vectors.iter().any(|v| v.name.is_none());
                        match check_using_names(statements, &dense, &sparse, unnamed) {
                            Ok(()) => {
                                if let Some(real) = observed_dim {
                                    let mut mismatch = false;
                                    for (vec_name, size) in dense_vector_sizes(&info) {
                                        if size as usize != real {
                                            mismatch = true;
                                            failed = true;
                                            ok_all = false;
                                            let label = if vec_name.is_empty() {
                                                "<default>".to_string()
                                            } else {
                                                vec_name
                                            };
                                            push(
                                                "topology",
                                                "fail",
                                                Some("QQL-BACKEND-DIMENSION-MISMATCH".to_string()),
                                                format!(
                                                    "[QQL-BACKEND-DIMENSION-MISMATCH] topology: collection '{collection}' vector '{label}' size={size} != embed dim={real}; fix: set EMBED_DIM={real} or recreate with VECTOR({real}, ...)"
                                                ),
                                            );
                                        }
                                    }
                                    if !mismatch {
                                        push(
                                            "topology",
                                            "ok",
                                            None,
                                            format!(
                                                "topology: USING names resolve on '{collection}' and dim={real} matches"
                                            ),
                                        );
                                    }
                                } else {
                                    push(
                                        "topology",
                                        "ok",
                                        None,
                                        format!("topology: USING names resolve on '{collection}'"),
                                    );
                                }
                            }
                            Err((code, msg)) => {
                                ok_all = false;
                                failed = true;
                                push(
                                    "topology",
                                    "fail",
                                    Some(code.clone()),
                                    format!(
                                        "[{code}] topology: {msg}; fix: use one of the listed vectors"
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            if ok_all && collections.len() > 1 {
                // Individual per-collection ok lines already pushed; nothing more.
            }
        } else {
            push(
                "topology",
                "skip",
                None,
                "topology: no executor available".to_string(),
            );
        }
    } else {
        push(
            "topology",
            "skip",
            None,
            "topology: skipped because the statement did not parse".to_string(),
        );
    }

    // Stage 4b: edge indexing state. qdrant-edge never indexes in the
    // background, so lag here explains "writes are not searchable yet" and
    // points at `qql edge optimize`.
    if use_edge
        && backend_reachable
        && let Some(exec) = executor.as_ref()
    {
        match collect_indexing_states(exec.client()).await {
            Ok(states) if states.is_empty() => push(
                "edge-indexing",
                "ok",
                None,
                "edge-indexing: no local collections yet".to_string(),
            ),
            Ok(states) => {
                for state in states {
                    match state.nudge {
                        Some(nudge) => push(
                            "edge-indexing",
                            "warn",
                            None,
                            format!("edge-indexing: {nudge}"),
                        ),
                        None => push(
                            "edge-indexing",
                            "ok",
                            None,
                            format!(
                                "edge-indexing: '{}' {} points, indexed {}",
                                state.collection,
                                state.points_count,
                                count_label(state.indexed_vectors_count)
                            ),
                        ),
                    }
                }
            }
            Err(e) => push(
                "edge-indexing",
                "warn",
                Some(e.code.to_string()),
                format!("edge-indexing: readout failed: {e}"),
            ),
        }
    }

    // Stage 5: backend doctor.
    if backend_reachable {
        push(
            "doctor",
            "ok",
            None,
            format!("doctor: backend at {url} answers SHOW COLLECTIONS"),
        );
    } else {
        failed = true;
        push("doctor", "fail", None, format!("doctor: {backend_note}"));
    }

    if let Some(exec) = executor.as_ref() {
        let _ = exec.close().await;
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": !failed,
                "operation": "check",
                "query": query,
                "stages": stages,
            })
        );
    } else if !quiet {
        for stage in &stages {
            let status = stage.get("status").and_then(|v| v.as_str()).unwrap_or("?");
            let message = stage.get("message").and_then(|v| v.as_str()).unwrap_or("");
            println!("[{status}] {message}");
        }
    }
    if failed {
        return Err("qql check failed; fix the first [fail] stage above".into());
    }
    Ok(())
}

fn parse_and_bind(
    query: &str,
    params: Option<&serde_json::Value>,
) -> Result<Vec<qql_core::ast::Stmt>, String> {
    let mut statements = qql_core::parser::Parser::parse_all(query).map_err(|e| e.to_string())?;
    if let Some(p) = params {
        for stmt in &mut statements {
            qql_core::params_json::bind_stmt_with_params(stmt, p).map_err(|e| e.to_string())?;
        }
    }
    Ok(statements)
}

fn statements_need_embeddings(stmts: &[qql_core::ast::Stmt]) -> bool {
    stmts.iter().any(stmt_needs_embeddings)
}

fn stmt_needs_embeddings(stmt: &qql_core::ast::Stmt) -> bool {
    use qql_core::ast::Stmt;
    match stmt {
        Stmt::Query(q) => query_stmt_needs_embeddings(q),
        Stmt::Upsert(u) => u.embedding.is_some() || !u.embed.is_empty(),
        _ => false,
    }
}

fn query_stmt_needs_embeddings(q: &qql_core::ast::QueryStmt) -> bool {
    q.ctes.iter().any(|c| query_stmt_needs_embeddings(&c.query))
        || query_expr_needs_embeddings(&q.expression)
}

fn query_expr_needs_embeddings(e: &qql_core::ast::QueryExpr) -> bool {
    use qql_core::ast::{QueryExpr, QueryInput, VectorValue};
    let input_needs = |input: &QueryInput| match input {
        QueryInput::Text { .. }
        | QueryInput::Image { .. }
        | QueryInput::Param(..)
        | QueryInput::PositionalParam(..) => true,
        QueryInput::Vector(VectorValue::Param(..) | VectorValue::PositionalParam(..)) => true,
        QueryInput::Vector(_) | QueryInput::Point(_) => false,
    };
    let prefetch_needs = |list: &[qql_core::ast::Prefetch]| {
        list.iter().any(|p| match &p.source {
            qql_core::ast::PrefetchSource::Query(q) => query_stmt_needs_embeddings(q),
            qql_core::ast::PrefetchSource::Cte(_) => false,
        })
    };
    match e {
        QueryExpr::Nearest {
            input, prefetch, ..
        } => input_needs(input) || prefetch_needs(prefetch),
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            positive.iter().any(input_needs)
                || negative.iter().any(input_needs)
                || prefetch_needs(prefetch)
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            pairs
                .iter()
                .any(|p| input_needs(&p.positive) || input_needs(&p.negative))
                || prefetch_needs(prefetch)
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            input_needs(target)
                || context
                    .iter()
                    .any(|p| input_needs(&p.positive) || input_needs(&p.negative))
                || prefetch_needs(prefetch)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            input_needs(target)
                || feedback.iter().any(|f| input_needs(&f.example))
                || prefetch_needs(prefetch)
        }
        QueryExpr::Hybrid { .. } => true,
        QueryExpr::Rerank {
            input, prefetch, ..
        } => input_needs(input) || prefetch_needs(prefetch),
        QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => prefetch_needs(prefetch),
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => false,
    }
}

fn stmt_collections(stmts: &[qql_core::ast::Stmt]) -> Vec<String> {
    use qql_core::ast::{QueryCollection, Stmt};
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !out.iter().any(|v| v == name) {
            out.push(name.to_string());
        }
    };
    for stmt in stmts {
        match stmt {
            Stmt::Query(q) => {
                if let QueryCollection::Explicit(name) = &q.collection {
                    push(name);
                }
                for cte in &q.ctes {
                    if let QueryCollection::Explicit(name) = &cte.query.collection {
                        push(name);
                    }
                }
            }
            Stmt::Scroll(s) => push(&s.collection),
            Stmt::Upsert(u) => push(&u.collection),
            Stmt::Delete(d) => push(&d.collection),
            Stmt::ClearPayload(s) => push(&s.collection),
            Stmt::DeletePayload(s) => push(&s.collection),
            Stmt::DeleteVector(s) => push(&s.collection),
            Stmt::UpdateVector(s) => push(&s.collection),
            Stmt::UpdatePayload(s) => push(&s.collection),
            Stmt::Count(c) => {
                if let QueryCollection::Explicit(name) = &c.collection {
                    push(name);
                }
            }
            Stmt::Facet(f) => {
                if let QueryCollection::Explicit(name) = &f.collection {
                    push(name);
                }
            }
            _ => {}
        }
    }
    out
}

fn check_using_names(
    stmts: &[qql_core::ast::Stmt],
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    for stmt in stmts {
        if let qql_core::ast::Stmt::Query(q) = stmt {
            check_query_using(q, dense, sparse, unnamed)?;
        }
    }
    Ok(())
}

fn check_query_using(
    q: &qql_core::ast::QueryStmt,
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    for cte in &q.ctes {
        check_query_using(&cte.query, dense, sparse, unnamed)?;
    }
    check_expr_using(&q.expression, dense, sparse, unnamed)
}

fn check_expr_using(
    e: &qql_core::ast::QueryExpr,
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    use qql_core::ast::{PrefetchSource, QueryExpr, VectorKind};
    let check_target =
        |target: &Option<qql_core::ast::VectorTarget>| -> Result<(), (String, String)> {
            if let Some(t) = target {
                let in_dense = dense.iter().any(|n| n == &t.name);
                let in_sparse = sparse.iter().any(|n| n == &t.name);
                if !in_dense && !in_sparse {
                    if t.kind.is_some() {
                        return Ok(());
                    }
                    let mut available: Vec<String> =
                        dense.iter().chain(sparse.iter()).cloned().collect();
                    if unnamed {
                        available.push("<default>".to_string());
                    }
                    return Err((
                        "QQL-UNKNOWN-VECTOR".to_string(),
                        format!(
                            "no vector named '{}'. Available vectors: {}",
                            t.name,
                            available.join(", ")
                        ),
                    ));
                }
                if let Some(kind) = t.kind {
                    let actual = if in_dense {
                        VectorKind::Dense
                    } else {
                        VectorKind::Sparse
                    };
                    if kind != actual {
                        return Err((
                            "QQL-VECTOR-KIND".to_string(),
                            format!(
                                "vector '{}' is {} on the collection",
                                t.name,
                                if in_dense { "dense" } else { "sparse" }
                            ),
                        ));
                    }
                }
            }
            Ok(())
        };
    let check_prefetches = |list: &[qql_core::ast::Prefetch]| -> Result<(), (String, String)> {
        for p in list {
            if let PrefetchSource::Query(q) = &p.source {
                check_query_using(q, dense, sparse, unnamed)?;
            }
        }
        Ok(())
    };
    match e {
        QueryExpr::Nearest {
            using, prefetch, ..
        }
        | QueryExpr::Recommend {
            using, prefetch, ..
        }
        | QueryExpr::Context {
            using, prefetch, ..
        }
        | QueryExpr::Discover {
            using, prefetch, ..
        }
        | QueryExpr::RelevanceFeedback {
            using, prefetch, ..
        } => {
            check_target(using)?;
            check_prefetches(prefetch)
        }
        QueryExpr::Rerank {
            using, prefetch, ..
        } => {
            check_target(using)?;
            check_prefetches(prefetch)
        }
        QueryExpr::Hybrid {
            dense_vector,
            sparse_vector,
            ..
        } => {
            if let Some(name) = dense_vector
                && !dense.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no dense vector named '{name}'. Available dense: {}",
                        dense.join(", ")
                    ),
                ));
            }
            if let Some(name) = sparse_vector
                && !sparse.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no sparse vector named '{name}'. Available sparse: {}",
                        sparse.join(", ")
                    ),
                ));
            }
            Ok(())
        }
        QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => check_prefetches(prefetch),
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => Ok(()),
    }
}
