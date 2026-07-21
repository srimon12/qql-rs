use crate::types::{EmbeddingJob, EmbeddingKind};
use qql_core::ast::{EmbedKind, EmbeddingSpec, QueryExpr, QueryInput, QueryStmt, Stmt, UpsertStmt};

pub fn extract_jobs(statement: &Stmt) -> Vec<EmbeddingJob> {
    let mut jobs = Vec::new();
    match statement {
        Stmt::Query(query) => extract_query_jobs(query, &mut jobs),
        Stmt::Upsert(upsert) => extract_upsert_jobs(upsert, &mut jobs),
        _ => {}
    }
    jobs
}

fn extract_query_jobs(query: &QueryStmt, jobs: &mut Vec<EmbeddingJob>) {
    extract_expression_jobs(&query.expression, jobs);

    for cte in &query.ctes {
        extract_query_jobs(&cte.query, jobs);
    }
}

fn collect_text_inputs(expr: &QueryExpr) -> Vec<&QueryInput> {
    match expr {
        QueryExpr::Nearest { input, .. } => vec![input],
        QueryExpr::Recommend {
            positive, negative, ..
        } => positive.iter().chain(negative.iter()).collect(),
        QueryExpr::Context { pairs, .. } => pairs
            .iter()
            .flat_map(|p| [&p.positive, &p.negative].into_iter())
            .collect(),
        QueryExpr::Discover {
            target, context, ..
        } => {
            let mut all: Vec<&QueryInput> = vec![target];
            all.extend(context.iter().flat_map(|p| [&p.positive, &p.negative]));
            all
        }
        QueryExpr::RelevanceFeedback { target, .. } => vec![target],
        QueryExpr::Rerank { input, .. } => vec![input],
        _ => Vec::new(),
    }
}

fn extract_expression_jobs(expr: &QueryExpr, jobs: &mut Vec<EmbeddingJob>) {
    if let QueryExpr::Hybrid { text, model, .. } = expr {
        jobs.push(EmbeddingJob {
            texts: vec![text.clone()],
            model: model.clone(),
            kind: EmbeddingKind::Dense,
            destinations: Vec::new(),
        });
        jobs.push(EmbeddingJob {
            texts: vec![text.clone()],
            model: None,
            kind: EmbeddingKind::Sparse,
            destinations: Vec::new(),
        });
        return;
    }

    for input in collect_text_inputs(expr) {
        if let QueryInput::Text { text, model } = input {
            jobs.push(EmbeddingJob {
                texts: vec![text.clone()],
                model: model.clone(),
                kind: EmbeddingKind::Dense,
                destinations: Vec::new(),
            });
        }
    }
}

fn extract_upsert_jobs(upsert: &UpsertStmt, jobs: &mut Vec<EmbeddingJob>) {
    let extract_texts = |field: &str| -> Vec<String> {
        upsert
            .points
            .iter()
            .filter_map(|point| {
                point
                    .payload
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(field))
                    .and_then(|(_, v)| match v {
                        qql_core::ast::Value::Str(s) => Some(s.clone()),
                        _ => None,
                    })
            })
            .collect()
    };

    if let Some(ref spec) = upsert.embedding {
        match spec {
            EmbeddingSpec::Dense { model, vector: _ } => {
                let texts = extract_texts("text");
                if !texts.is_empty() {
                    jobs.push(EmbeddingJob {
                        texts,
                        model: model.clone(),
                        kind: EmbeddingKind::Dense,
                        destinations: Vec::new(),
                    });
                }
            }
            EmbeddingSpec::Hybrid {
                dense_model,
                sparse_model,
                ..
            } => {
                let texts = extract_texts("text");
                if !texts.is_empty() {
                    jobs.push(EmbeddingJob {
                        texts: texts.clone(),
                        model: dense_model.clone(),
                        kind: EmbeddingKind::Dense,
                        destinations: Vec::new(),
                    });
                    jobs.push(EmbeddingJob {
                        texts,
                        model: sparse_model.clone(),
                        kind: EmbeddingKind::Sparse,
                        destinations: Vec::new(),
                    });
                }
            }
        }
    }

    for directive in &upsert.embed {
        let texts = extract_texts(&directive.source_field);
        if !texts.is_empty() {
            let (kind, model) = match &directive.kind {
                EmbedKind::Dense { model } => (EmbeddingKind::Dense, model.clone()),
                EmbedKind::Sparse { model } => (EmbeddingKind::Sparse, model.clone()),
            };
            jobs.push(EmbeddingJob {
                texts,
                model,
                kind,
                destinations: Vec::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

    #[test]
    fn query_text_extracts_job() {
        let stmt = Parser::parse("QUERY 'search query' FROM docs LIMIT 5;").unwrap();
        let jobs = extract_jobs(&stmt);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].texts, vec!["search query"]);
        assert_eq!(jobs[0].kind, EmbeddingKind::Dense);
        assert_eq!(jobs[0].model, None);
    }

    #[test]
    fn query_nearest_text_with_model() {
        let stmt = Parser::parse("QUERY TEXT 'hello' MODEL 'nomic-embed-text' FROM docs;").unwrap();
        let jobs = extract_jobs(&stmt);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].model.as_deref(), Some("nomic-embed-text"));
    }

    #[test]
    fn vector_query_no_job() {
        let stmt = Parser::parse("QUERY NEAREST VECTOR [0.1, 0.2] FROM docs;").unwrap();
        let jobs = extract_jobs(&stmt);
        assert!(jobs.is_empty());
    }

    #[test]
    fn hybrid_extracts_dense_and_sparse() {
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'database' DENSE dense SPARSE sparse FUSION RRF FROM docs;",
        )
        .unwrap();
        let jobs = extract_jobs(&stmt);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].kind, EmbeddingKind::Dense);
        assert_eq!(jobs[1].kind, EmbeddingKind::Sparse);
    }

    #[test]
    fn upsert_embedding_dense_extracts_texts() {
        let stmt = Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'}, {id: 2, text: 'world'} USING DENSE MODEL 'nomic';",
        )
        .unwrap();
        let jobs = extract_jobs(&stmt);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].texts.len(), 2);
        assert_eq!(jobs[0].model.as_deref(), Some("nomic"));
    }

    #[test]
    fn upsert_embed_directive() {
        let stmt = Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, title: 'doc one'}, {id: 2, title: 'doc two'} EMBED title INTO dense_vec USING MODEL 'embed';",
        )
        .unwrap();
        let jobs = extract_jobs(&stmt);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].texts, vec!["doc one", "doc two"]);
        assert_eq!(jobs[0].model.as_deref(), Some("embed"));
    }

    #[test]
    fn order_by_sample_no_jobs() {
        for source in [
            "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
            "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
            "QUERY POINTS (42) FROM docs;",
        ] {
            let stmt = Parser::parse(source).unwrap();
            let jobs = extract_jobs(&stmt);
            assert!(jobs.is_empty(), "expected no jobs for: {}", source);
        }
    }
}
