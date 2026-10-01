//! `qql check` — format, explain, embed probe, topology, doctor.

use super::convert::source_is_canonical;
use super::edge::{collect_indexing_states, count_label};
#[cfg(feature = "rest")]
use super::runtime::probe_embed_dim;
use super::runtime::{
    classify_backend_failure, dense_vector_sizes, executor, explain_query_bound,
    resolve_embed_settings, statements_need_embeddings,
};
use std::collections::HashMap;

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
            let mut topologies: HashMap<String, CollectionTopology> = HashMap::new();
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
                        if let Some(real) = observed_dim {
                            for (vec_name, size) in dense_vector_sizes(&info) {
                                if size as usize != real {
                                    ok_all = false;
                                    failed = true;
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
                        }
                        topologies.insert(collection.clone(), CollectionTopology::from_info(&info));
                    }
                }
            }
            // Every USING target is validated against its own collection: a
            // prefetch sub-query with an explicit FROM is checked against that
            // collection, everything else inherits the enclosing one.
            if let Err((code, msg)) = check_using_names(statements, &topologies) {
                ok_all = false;
                failed = true;
                push(
                    "topology",
                    "fail",
                    Some(code.clone()),
                    format!("[{code}] topology: {msg}; fix: use one of the listed vectors"),
                );
            }
            if ok_all {
                let summary = match observed_dim {
                    Some(real) => format!(
                        "topology: USING names resolve on {} collection(s) and dim={real} matches",
                        collections.len()
                    ),
                    None => format!(
                        "topology: USING names resolve on {} collection(s)",
                        collections.len()
                    ),
                };
                push("topology", "ok", None, summary);
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

/// Topology of one collection: vector names and whether an unnamed default
/// dense vector exists.
struct CollectionTopology {
    dense: Vec<String>,
    sparse: Vec<String>,
    unnamed: bool,
}

impl CollectionTopology {
    fn from_info(info: &qql::client::CollectionInfo) -> Self {
        Self {
            dense: info
                .schema
                .vectors
                .iter()
                .filter_map(|v| v.name.clone())
                .collect(),
            sparse: info
                .schema
                .sparse_vectors
                .iter()
                .map(|v| v.name.clone())
                .collect(),
            unnamed: info.schema.vectors.iter().any(|v| v.name.is_none()),
        }
    }
}

fn push_collection(out: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !out.iter().any(|v| v == name) {
        out.push(name.to_string());
    }
}

/// Every collection a statement touches, including prefetch sub-queries,
/// CTEs, and BATCH members. Order-stable and deduplicated.
fn stmt_collections(stmts: &[qql_core::ast::Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in stmts {
        collect_stmt_collections(stmt, &mut out);
    }
    out
}

fn collect_stmt_collections(stmt: &qql_core::ast::Stmt, out: &mut Vec<String>) {
    use qql_core::ast::Stmt;
    match stmt {
        Stmt::Query(q) => collect_query_collections(q, out),
        Stmt::Scroll(s) => push_collection(out, &s.collection),
        Stmt::Upsert(u) => push_collection(out, &u.collection),
        Stmt::Delete(d) => push_collection(out, &d.collection),
        Stmt::ClearPayload(s) => push_collection(out, &s.collection),
        Stmt::DeletePayload(s) => push_collection(out, &s.collection),
        Stmt::DeleteVector(s) => push_collection(out, &s.collection),
        Stmt::UpdateVector(s) => push_collection(out, &s.collection),
        Stmt::UpdatePayload(s) => push_collection(out, &s.collection),
        Stmt::Count(c) => {
            if let qql_core::ast::QueryCollection::Explicit(name) = &c.collection {
                push_collection(out, name);
            }
        }
        Stmt::Facet(f) => {
            if let qql_core::ast::QueryCollection::Explicit(name) = &f.collection {
                push_collection(out, name);
            }
        }
        Stmt::Batch(b) => {
            for member in &b.statements {
                collect_stmt_collections(member, out);
            }
        }
        _ => {}
    }
}

fn collect_query_collections(q: &qql_core::ast::QueryStmt, out: &mut Vec<String>) {
    if let qql_core::ast::QueryCollection::Explicit(name) = &q.collection {
        push_collection(out, name);
    }
    for cte in &q.ctes {
        collect_query_collections(&cte.query, out);
    }
    collect_expr_collections(&q.expression, out);
}

fn collect_expr_collections(e: &qql_core::ast::QueryExpr, out: &mut Vec<String>) {
    use qql_core::ast::QueryExpr;
    let prefetch = match e {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => prefetch,
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => return,
    };
    for p in prefetch {
        if let qql_core::ast::PrefetchSource::Query(sub) = &p.source {
            collect_query_collections(sub, out);
        }
    }
}

fn check_using_names(
    stmts: &[qql_core::ast::Stmt],
    topologies: &HashMap<String, CollectionTopology>,
) -> Result<(), (String, String)> {
    use qql_core::ast::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Query(q) => {
                let collection = match &q.collection {
                    qql_core::ast::QueryCollection::Explicit(name) => Some(name.as_str()),
                    qql_core::ast::QueryCollection::Inherited => None,
                };
                check_query_using(q, collection, topologies)?;
            }
            Stmt::Batch(b) => check_using_names(&b.statements, topologies)?,
            _ => {}
        }
    }
    Ok(())
}

fn check_query_using(
    q: &qql_core::ast::QueryStmt,
    collection: Option<&str>,
    topologies: &HashMap<String, CollectionTopology>,
) -> Result<(), (String, String)> {
    for cte in &q.ctes {
        let cte_collection = match &cte.query.collection {
            qql_core::ast::QueryCollection::Explicit(name) => Some(name.as_str()),
            qql_core::ast::QueryCollection::Inherited => collection,
        };
        check_query_using(&cte.query, cte_collection, topologies)?;
    }
    check_expr_using(&q.expression, collection, topologies)
}

fn check_expr_using(
    e: &qql_core::ast::QueryExpr,
    collection: Option<&str>,
    topologies: &HashMap<String, CollectionTopology>,
) -> Result<(), (String, String)> {
    use qql_core::ast::{PrefetchSource, QueryCollection, QueryExpr, VectorKind};
    // No fetched topology for this target (inherited or unknown): collection
    // lookups are already reported separately, so there is nothing to check.
    let Some(topo) = collection.and_then(|name| topologies.get(name)) else {
        return Ok(());
    };
    let check_target =
        |target: &Option<qql_core::ast::VectorTarget>| -> Result<(), (String, String)> {
            if let Some(t) = target {
                let in_dense = topo.dense.iter().any(|n| n == &t.name);
                let in_sparse = topo.sparse.iter().any(|n| n == &t.name);
                if !in_dense && !in_sparse {
                    if t.kind.is_some() {
                        return Ok(());
                    }
                    let mut available: Vec<String> =
                        topo.dense.iter().chain(topo.sparse.iter()).cloned().collect();
                    if topo.unnamed {
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
            if let PrefetchSource::Query(sub) = &p.source {
                // A prefetch FROM a different collection validates against
                // that collection; a FROM-less sub-query inherits the parent.
                let sub_collection = match &sub.collection {
                    QueryCollection::Explicit(name) => Some(name.as_str()),
                    QueryCollection::Inherited => collection,
                };
                check_query_using(sub, sub_collection, topologies)?;
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
        }
        | QueryExpr::Rerank {
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
                && !topo.dense.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no dense vector named '{name}'. Available dense: {}",
                        topo.dense.join(", ")
                    ),
                ));
            }
            if let Some(name) = sparse_vector
                && !topo.sparse.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no sparse vector named '{name}'. Available sparse: {}",
                        topo.sparse.join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

    fn topology(dense: &[&str], sparse: &[&str], unnamed: bool) -> CollectionTopology {
        CollectionTopology {
            dense: dense.iter().map(|name| name.to_string()).collect(),
            sparse: sparse.iter().map(|name| name.to_string()).collect(),
            unnamed,
        }
    }

    #[test]
    fn stmt_collections_walks_prefetch_sub_queries() {
        let stmt = Parser::parse(
            "QUERY [0.1] FROM a PREFETCH (QUERY [0.2] FROM b USING b_vec LIMIT 5) LIMIT 3;",
        )
        .expect("parse");
        assert_eq!(stmt_collections(&[stmt]), ["a", "b"]);
    }

    #[test]
    fn cross_collection_prefetch_validates_against_its_own_collection() {
        let stmt = Parser::parse(
            "QUERY [0.1] FROM a PREFETCH (QUERY [0.2] FROM b USING b_vec LIMIT 5) LIMIT 3;",
        )
        .expect("parse");
        let mut topologies = HashMap::new();
        topologies.insert("a".to_string(), topology(&["a_vec"], &[], false));
        topologies.insert("b".to_string(), topology(&["b_vec"], &[], false));
        check_using_names(std::slice::from_ref(&stmt), &topologies)
            .expect("b_vec resolves on collection b, not a");

        // When b lacks the name, the failure reports b's own vector list.
        topologies.insert("b".to_string(), topology(&["other"], &[], false));
        let (code, message) =
            check_using_names(&[stmt], &topologies).expect_err("b_vec must not resolve");
        assert_eq!(code, "QQL-UNKNOWN-VECTOR");
        assert!(message.contains("b_vec"), "{message}");
    }

    #[test]
    fn inherited_cte_prefetch_uses_the_parent_collection() {
        let stmt = Parser::parse(
            "WITH c AS (QUERY [0.2] USING a_vec LIMIT 5) \
             QUERY [0.1] FROM a USING a_vec PREFETCH (c) LIMIT 3;",
        )
        .expect("parse");
        let mut topologies = HashMap::new();
        topologies.insert("a".to_string(), topology(&["a_vec"], &[], false));
        check_using_names(&[stmt], &topologies).expect("inherited collection resolves");
    }
}
