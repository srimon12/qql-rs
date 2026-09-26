//! Focused regression tests for embedding resolution (EMBED-001..005).

use async_trait::async_trait;
use qql_core::ast::{
    FusionMethod, PointEntry, PointId, PointVectors, Prefetch, PrefetchSource, QueryExpr,
    QueryInput, QueryStmt, Stmt, UpsertPoint, UpsertStmt, VectorValue,
};
use qql_core::error::QqlError;
use qql_core::parser::Parser;
use std::sync::{Arc, Mutex};

use crate::embedder::Embedder;
use crate::resolve::resolve_embeddings;
use crate::sparse::SparseVector;
use crate::topology::{TopologyNames, query_needs_kind_resolution, resolve_query_vector_kinds};

struct MockEmbedder {
    dense_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, text)
    sparse_query_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, text)
    sparse_document_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, text)
    multi_calls: Arc<Mutex<Vec<(String, String)>>>,
    image_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, source)
    dense_batch_override: Option<Vec<Vec<f32>>>,
    sparse_batch_override: Option<Vec<SparseVector>>,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self {
            dense_calls: Arc::new(Mutex::new(Vec::new())),
            sparse_query_calls: Arc::new(Mutex::new(Vec::new())),
            sparse_document_calls: Arc::new(Mutex::new(Vec::new())),
            multi_calls: Arc::new(Mutex::new(Vec::new())),
            image_calls: Arc::new(Mutex::new(Vec::new())),
            dense_batch_override: None,
            sparse_batch_override: None,
        }
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        self.dense_calls
            .lock()
            .unwrap()
            .push((model.to_string(), text.to_string()));
        Ok(vec![1.0, 2.0, 3.0])
    }

    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        self.sparse_query_calls
            .lock()
            .unwrap()
            .push((model.to_string(), text.to_string()));
        Ok(SparseVector {
            indices: vec![1],
            values: vec![1.0],
        })
    }

    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        self.sparse_document_calls
            .lock()
            .unwrap()
            .push((model.to_string(), text.to_string()));
        Ok(SparseVector {
            indices: vec![1],
            values: vec![1.0],
        })
    }

    async fn embed_sparse_document_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        if let Some(ref override_vecs) = self.sparse_batch_override {
            return Ok(override_vecs.clone());
        }
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            out.push(self.embed_sparse_document(text, model).await?);
        }
        Ok(out)
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        for text in texts {
            self.dense_calls
                .lock()
                .unwrap()
                .push((model.to_string(), text.clone()));
        }
        if let Some(ref override_vecs) = self.dense_batch_override {
            return Ok(override_vecs.clone());
        }
        Ok(texts.iter().map(|_| vec![1.0, 2.0, 3.0]).collect())
    }

    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        self.multi_calls
            .lock()
            .unwrap()
            .push((model.to_string(), text.to_string()));
        Ok(vec![vec![0.1, 0.2], vec![0.3, 0.4], vec![0.5, 0.6]])
    }

    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        self.image_calls
            .lock()
            .unwrap()
            .push((model.to_string(), source.to_string()));
        Ok(vec![0.5, 0.5, 0.5])
    }
}

#[tokio::test]
async fn rerank_uses_model_not_vector_name() {
    let mut stmt = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense AS DENSE LIMIT 100) \
         QUERY RERANK TEXT 'rerank-me' MODEL 'colbert-v2' FROM docs USING colbert AS DENSE PREFETCH (c) LIMIT 10;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let calls = mock.dense_calls.lock().unwrap();
    // At least one call must use the rerank model, never only the vector name as model.
    assert!(
        calls
            .iter()
            .any(|(m, t)| m == "colbert-v2" && t == "rerank-me"),
        "expected model=colbert-v2 for rerank text, got: {:?}",
        *calls
    );
    assert!(
        !calls
            .iter()
            .any(|(m, t)| m == "colbert" && t == "rerank-me"),
        "vector name must not be used as embedding model: {:?}",
        *calls
    );
}

#[tokio::test]
async fn upsert_batch_cardinality_mismatch_errors() {
    let mut stmt = Stmt::Upsert(Box::new(UpsertStmt {
        collection: "docs".into(),
        points: vec![
            PointEntry::Inline(UpsertPoint {
                id: PointId::Number(1),
                vectors: None,
                payload: vec![("text".into(), qql_core::ast::Value::Str("a".into()))],
            }),
            PointEntry::Inline(UpsertPoint {
                id: PointId::Number(2),
                vectors: None,
                payload: vec![("text".into(), qql_core::ast::Value::Str("b".into()))],
            }),
        ],
        embedding: Some(qql_core::ast::EmbeddingSpec::Dense {
            model: Some("m".into()),
            vector: Some("dense".into()),
            field: None,
        }),
        embed: vec![],
        update_filter: None,
        update_mode: None,
        shard_key: None,
        wait: None,
    }));
    let mock = MockEmbedder {
        dense_batch_override: Some(vec![vec![1.0]]), // only 1 vector for 2 texts
        ..Default::default()
    };
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(
        err.message
            .contains("embed_dense_batch returned 1 vectors for 2 texts"),
        "expected the dense batch entry point in the cardinality error, got: {}",
        err.message
    );
}

#[tokio::test]
async fn unnamed_vector_topology_conflict_rejected() {
    let mut stmt = Stmt::Upsert(Box::new(UpsertStmt {
        collection: "docs".into(),
        points: vec![PointEntry::Inline(UpsertPoint {
            id: PointId::Number(1),
            vectors: Some(PointVectors::Unnamed(VectorValue::Dense(vec![0.1, 0.2]))),
            payload: vec![("text".into(), qql_core::ast::Value::Str("hello".into()))],
        })],
        embedding: Some(qql_core::ast::EmbeddingSpec::Dense {
            model: Some("m".into()),
            vector: Some("dense".into()),
            field: None,
        }),
        embed: vec![],
        update_filter: None,
        update_mode: None,
        shard_key: None,
        wait: None,
    }));
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(
        err.message.contains("unnamed vector") || err.message.contains("named vector"),
        "expected topology error, got: {}",
        err
    );
}

#[tokio::test]
async fn upsert_multi_vector_spec_calls_embed_multi() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'late interaction'} \
         USING MULTI MODEL 'bge-m3' VECTOR colbert;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let multi = mock.multi_calls.lock().unwrap();
    assert_eq!(multi.len(), 1);
    assert_eq!(multi[0].0, "bge-m3");
    assert_eq!(multi[0].1, "late interaction");
    let Stmt::Upsert(u) = &stmt else {
        panic!("expected upsert");
    };
    let PointEntry::Inline(point) = &u.points[0] else {
        panic!("expected inline point");
    };
    match &point.vectors {
        Some(PointVectors::Named(list)) => {
            let (_, v) = list
                .iter()
                .find(|(n, _)| n == "colbert")
                .expect("colbert vector");
            assert!(matches!(v, VectorValue::MultiDense(rows) if !rows.is_empty()));
        }
        other => panic!("expected named multi vector, got {other:?}"),
    }
}

#[tokio::test]
async fn image_query_calls_embed_image() {
    let mut stmt = Parser::parse(
        "QUERY IMAGE '/tmp/x.jpg' MODEL 'clip-vision' FROM products USING image AS DENSE LIMIT 5;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let images = mock.image_calls.lock().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].0, "clip-vision");
    assert_eq!(images[0].1, "/tmp/x.jpg");
    let Stmt::Query(q) = &stmt else {
        panic!("expected query");
    };
    match &q.expression {
        QueryExpr::Nearest {
            input: QueryInput::Vector(VectorValue::Dense(v)),
            ..
        } => assert_eq!(v, &vec![0.5, 0.5, 0.5]),
        other => panic!("expected dense from image, got {other:?}"),
    }
}

#[tokio::test]
async fn upsert_image_spec_calls_embed_image() {
    let mut stmt = Parser::parse(
        "UPSERT INTO products VALUES {id: 1, image: '/a.jpg'} \
         USING IMAGE MODEL 'clip-vision' ON FIELD image INTO image;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let images = mock.image_calls.lock().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].1, "/a.jpg");
    let Stmt::Upsert(u) = &stmt else {
        panic!("expected upsert");
    };
    let PointEntry::Inline(point) = &u.points[0] else {
        panic!("expected inline point");
    };
    match &point.vectors {
        Some(PointVectors::Named(list)) => {
            let (_, v) = list.iter().find(|(n, _)| n == "image").expect("image vec");
            assert!(matches!(v, VectorValue::Dense(_)));
        }
        other => panic!("expected named dense, got {other:?}"),
    }
}

#[tokio::test]
async fn as_multi_query_calls_embed_multi() {
    let mut stmt =
        Parser::parse("QUERY TEXT 'q' FROM docs USING colbert AS MULTI LIMIT 5;").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let multi = mock.multi_calls.lock().unwrap();
    assert_eq!(multi.len(), 1);
    assert_eq!(multi[0].1, "q");
    let dense = mock.dense_calls.lock().unwrap();
    assert!(dense.is_empty(), "multi path must not use dense embed");
}

#[tokio::test]
async fn upsert_sparse_spec_delegates_model_to_embedder() {
    // The permissive mock accepts any sparse model, so this pins that the
    // upsert sparse path forwards the MODEL clause to the embedder's document
    // entry point (rather than rejecting it in `resolve`). The default
    // reject gate is pinned on the query path by
    // `query_sparse_model_rejected_by_default_embedder` below.
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING SPARSE MODEL 'splade';",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let calls = mock.sparse_document_calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "expected one sparse document call, got {calls:?}"
    );
    assert_eq!(calls[0].0, "splade", "MODEL clause must reach the embedder");
    assert_eq!(calls[0].1, "hello");

    // The resolved sparse vector lands on the default sparse target.
    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected upsert");
    };
    let PointEntry::Inline(point) = &upsert.points[0] else {
        panic!("expected inline point");
    };
    let Some(PointVectors::Named(list)) = &point.vectors else {
        panic!("expected named vectors, got {:?}", point.vectors);
    };
    assert!(
        list.iter().any(|(name, value)| name == "sparse"
            && matches!(value, VectorValue::Sparse { indices, values }
                if indices == &vec![1] && values == &vec![1.0])),
        "expected the sparse target to carry the embedder output, got {list:?}"
    );
    assert!(
        mock.sparse_query_calls.lock().unwrap().is_empty(),
        "upsert must use the document sparse role, not the query role"
    );
}

#[tokio::test]
async fn query_sparse_model_rejected_by_default_embedder() {
    let mut stmt = Parser::parse(
        "QUERY TEXT 'hello' MODEL 'splade' FROM docs USING sparse AS SPARSE LIMIT 10",
    )
    .unwrap();
    let err = resolve_embeddings(&mut stmt, &DefaultEmbedder)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING-SPARSE");
    assert!(err.message.contains("splade"), "got: {}", err.message);
}

#[tokio::test]
async fn hybrid_query_threads_model_into_sparse_leg() {
    // C2 repro: the dense leg batches with `Hybrid.model`, so the sparse leg
    // must receive the same model (not a hardcoded "default").
    let mut stmt = Parser::parse(
        "QUERY HYBRID TEXT 'q' MODEL 'splade' DENSE d SPARSE s FUSION RRF FROM docs LIMIT 10",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let sparse = mock.sparse_query_calls.lock().unwrap();
    assert_eq!(sparse.len(), 1, "expected one sparse leg call");
    assert_eq!(sparse[0].0, "splade", "sparse leg must see Hybrid.model");
    assert_eq!(sparse[0].1, "q");
    let dense = mock.dense_calls.lock().unwrap();
    assert!(
        dense.iter().any(|(m, t)| m == "splade" && t == "q"),
        "dense leg must see Hybrid.model, got: {dense:?}"
    );
}

#[tokio::test]
async fn empty_dense_vector_rejected_on_query_path() {
    // C3b repro: multi/image legs reject empty vectors; the dense leg must too.
    let mut stmt = Parser::parse("QUERY 'hello' FROM docs LIMIT 10").unwrap();
    let mock = MockEmbedder {
        dense_batch_override: Some(vec![Vec::new()]),
        ..Default::default()
    };
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(
        err.message.contains("empty"),
        "expected empty-vector error, got: {}",
        err.message
    );
}

#[tokio::test]
async fn chained_cte_embeddings_not_duplicated() {
    let mut stmt = Parser::parse(
        "WITH a AS (QUERY TEXT 'first' USING dense AS DENSE LIMIT 10), \
         b AS (QUERY TEXT 'second' USING dense AS DENSE PREFETCH (a) LIMIT 10) \
         QUERY TEXT 'third' FROM docs USING dense AS DENSE PREFETCH (b) LIMIT 10;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let calls = mock.dense_calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        3,
        "expected exactly 3 dense embedding jobs (first, second, third), got: {:?}",
        *calls
    );
    assert_eq!(calls[0].1, "first");
    assert_eq!(calls[1].1, "second");
    assert_eq!(calls[2].1, "third");
}

// ── Inference OPTIONS / unbound placeholders on the client-embed path ─────

#[tokio::test]
async fn text_options_fail_closed_before_client_embedding() {
    // OPTIONS is a server-side inference directive; the Embedder trait has no
    // options channel, so silently dropping it would embed differently than
    // the server path. Fail closed, before any embedder call.
    let mut stmt =
        Parser::parse("QUERY TEXT 'hello' OPTIONS {truncate: false} FROM docs LIMIT 5;").unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(err.message.contains("OPTIONS"), "got: {}", err.message);
    assert!(
        mock.dense_calls.lock().unwrap().is_empty(),
        "the options-carrying input must not be half-embedded"
    );
}

#[tokio::test]
async fn image_options_fail_closed_before_client_embedding() {
    let mut stmt = Parser::parse(
        "QUERY IMAGE '/x.jpg' OPTIONS {alpha: 1} FROM docs USING i AS DENSE LIMIT 5;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(err.message.contains("OPTIONS") && err.message.contains("IMAGE"));
    assert!(mock.image_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn object_options_still_pass_through_to_server_inference() {
    // OBJECT inputs are never client-embedded: OPTIONS keeps lowering to the
    // backend inference request and no embedder call happens.
    let mut stmt =
        Parser::parse("QUERY OBJECT {text: 'x'} OPTIONS {truncate: false} FROM docs LIMIT 5;")
            .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    assert!(mock.dense_calls.lock().unwrap().is_empty());
    assert!(mock.image_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unbound_text_param_fails_closed_instead_of_embedding_empty() {
    let mut stmt = Parser::parse("QUERY TEXT :q FROM docs LIMIT 5;").unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(err.message.contains("unbound"), "got: {}", err.message);

    // The placeholder survives in the AST for a later bind.
    let Stmt::Query(query) = &stmt else {
        panic!("expected query");
    };
    let QueryExpr::Nearest {
        input: QueryInput::Text { text_param, .. },
        ..
    } = &query.expression
    else {
        panic!("expected Nearest TEXT input");
    };
    assert_eq!(text_param.as_deref(), Some(":q"));

    // The sparse leg used to silently rewrite the placeholder to an empty
    // sparse vector; it must fail closed identically.
    let mut sparse_stmt =
        Parser::parse("QUERY TEXT :q FROM docs USING s AS SPARSE LIMIT 5;").unwrap();
    let err = resolve_embeddings(&mut sparse_stmt, &mock)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(mock.sparse_query_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unbound_hybrid_text_param_fails_closed() {
    let mut stmt =
        Parser::parse("QUERY HYBRID TEXT :q DENSE d SPARSE s FUSION RRF FROM docs LIMIT 10;")
            .unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(mock.dense_calls.lock().unwrap().is_empty());
    assert!(mock.sparse_query_calls.lock().unwrap().is_empty());
}

// ── UPSERT target selection and duplicate targets ─────────────────────────

#[tokio::test]
async fn upsert_fallback_uses_the_same_field_priority_as_using_specs() {
    // A `title`-only point used to get no vector in fallback mode while the
    // same point under `USING DENSE` embedded from `title`. Both paths must
    // search the same ordered default field set.
    let mut fallback =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, title: 'header text'}").unwrap();
    let fallback_mock = MockEmbedder::default();
    resolve_embeddings(&mut fallback, &fallback_mock)
        .await
        .unwrap();
    {
        let calls = fallback_mock.dense_calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "title-only point must embed, got {calls:?}");
        assert_eq!(calls[0].1, "header text");
    }

    // Fixed priority, not payload order: `content` precedes `text` in the
    // payload but `text` wins.
    let mut ordered =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, content: 'secondary', text: 'primary'}")
            .unwrap();
    let ordered_mock = MockEmbedder::default();
    resolve_embeddings(&mut ordered, &ordered_mock)
        .await
        .unwrap();
    let calls = ordered_mock.dense_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "primary");
}

#[tokio::test]
async fn duplicate_target_across_spec_and_directive_errors() {
    // The spec's `INTO vec1` and the directive's `INTO vec1` used to clobber
    // silently: the second clause overwrote the first vector.
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} \
         USING DENSE MODEL 'm' INTO vec1 EMBED text INTO vec1 USING SPARSE",
    )
    .unwrap();
    let err = resolve_embeddings(&mut stmt, &MockEmbedder::default())
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(
        err.message.contains("duplicate target vector 'vec1'"),
        "got: {}",
        err.message
    );
}

#[tokio::test]
async fn duplicate_target_across_directives_errors() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} \
         EMBED text INTO vec1 USING SPARSE, text INTO vec1 USING DENSE",
    )
    .unwrap();
    let err = resolve_embeddings(&mut stmt, &MockEmbedder::default())
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(err.message.contains("duplicate target vector 'vec1'"));
}

#[tokio::test]
async fn distinct_targets_across_clauses_still_embed() {
    // The cross-clause duplicate check must not reject genuinely distinct
    // targets in the same statement.
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'header', body: 'content'} \
         USING DENSE MODEL 'm' INTO vec1 EMBED body INTO vec2 USING SPARSE",
    )
    .unwrap();
    resolve_embeddings(&mut stmt, &MockEmbedder::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn diagnostics_union_covers_later_points() {
    // The "Found fields" hint used to list only the first point's keys, which
    // misleads triage when a later point carries the expected field.
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, score: 1}, {id: 2, text: 'hi'} USING DENSE MODEL 'm'",
    )
    .unwrap();
    // Point 2 has `text`, so the spec matches it; nothing errors. A batch
    // where only a later point has a *non-default* field must still error.
    resolve_embeddings(&mut stmt, &MockEmbedder::default())
        .await
        .unwrap();

    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, score: 1}, {id: 2, rank: 2} USING DENSE MODEL 'm'",
    )
    .unwrap();
    let err = resolve_embeddings(&mut stmt, &MockEmbedder::default())
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(
        err.message.contains("score") && err.message.contains("rank"),
        "diagnostic must union later points' fields, got: {}",
        err.message
    );
}

// ── Batch cardinality messages ────────────────────────────────────────────

#[tokio::test]
async fn sparse_batch_cardinality_error_names_the_sparse_entry_point() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'a'}, {id: 2, text: 'b'} USING SPARSE",
    )
    .unwrap();
    let mock = MockEmbedder {
        sparse_batch_override: Some(vec![SparseVector {
            indices: vec![1],
            values: vec![1.0],
        }]),
        ..Default::default()
    };
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(
        err.message
            .contains("embed_sparse_document_batch returned 1 vectors for 2 texts"),
        "got: {}",
        err.message
    );
}

// ── Nesting depth guard ───────────────────────────────────────────────────

/// `depth` nested `PREFETCH (QUERY …)` stages around one embeddable leaf.
fn nested_prefetch_query(depth: usize) -> QueryStmt {
    fn leaf() -> QueryStmt {
        match Parser::parse("QUERY TEXT 'leaf' FROM docs USING dense AS DENSE LIMIT 5") {
            Ok(Stmt::Query(query)) => *query,
            other => panic!("expected query, got {other:?}"),
        }
    }
    let mut query = leaf();
    for _ in 0..depth {
        let mut next = leaf();
        let QueryExpr::Nearest { prefetch, .. } = &mut next.expression else {
            panic!("expected Nearest");
        };
        prefetch.push(Prefetch {
            source: PrefetchSource::Query(Box::new(query)),
            filter: None,
            score_threshold: None,
            lookup: None,
        });
        query = next;
    }
    query
}

/// `depth` nested `FUSION` prefetch stages around a target-less leaf, so the
/// kind predicate must walk the whole chain instead of stopping at the root.
fn nested_fusion_query(depth: usize) -> QueryStmt {
    let mut query = match Parser::parse("QUERY POINTS (42) FROM docs;") {
        Ok(Stmt::Query(query)) => *query,
        other => panic!("expected query, got {other:?}"),
    };
    for _ in 0..depth {
        let mut next = match Parser::parse("QUERY POINTS (42) FROM docs;") {
            Ok(Stmt::Query(query)) => *query,
            other => panic!("expected query, got {other:?}"),
        };
        next.expression = QueryExpr::Fusion {
            method: FusionMethod::Rrf,
            prefetch: vec![Prefetch {
                source: PrefetchSource::Query(Box::new(query)),
                filter: None,
                score_threshold: None,
                lookup: None,
            }],
        };
        query = next;
    }
    query
}

#[tokio::test]
async fn deeply_nested_prefetch_fails_closed_instead_of_overflowing() {
    let limit = crate::topology::MAX_NESTING_DEPTH;
    let mock = MockEmbedder::default();

    // The synchronous job collect walk fails closed past the guard.
    let mut stmt = Stmt::Query(Box::new(nested_prefetch_query(limit + 1)));
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-NESTING");
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);

    // The topology configure walk fails closed on the same shape.
    let mut query = nested_prefetch_query(limit + 1);
    let err = resolve_query_vector_kinds(
        "docs",
        &mut query,
        &TopologyNames {
            dense: vec!["dense".into()],
            sparse: Vec::new(),
            multivector: Vec::new(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-NESTING");

    // Just under the guard still embeds.
    let mut ok_stmt = Stmt::Query(Box::new(nested_prefetch_query(limit - 1)));
    resolve_embeddings(&mut ok_stmt, &mock).await.unwrap();
}

#[test]
fn kind_predicate_walks_deep_chains_iteratively() {
    // No targets anywhere in the chain, so the predicate must visit every
    // node; a recursive walk with large frames would not survive this depth.
    assert!(!query_needs_kind_resolution(&nested_fusion_query(2048)));

    // A target deep in the chain is still found.
    let mut query = nested_fusion_query(3);
    let QueryExpr::Fusion { prefetch, .. } = &mut query.expression else {
        panic!("expected Fusion");
    };
    let PrefetchSource::Query(inner) = &mut prefetch[0].source else {
        panic!("expected inline prefetch query");
    };
    let QueryExpr::Fusion { prefetch, .. } = &mut inner.expression else {
        panic!("expected Fusion");
    };
    let PrefetchSource::Query(inner) = &mut prefetch[0].source else {
        panic!("expected inline prefetch query");
    };
    inner.expression = QueryExpr::Nearest {
        input: QueryInput::Text {
            text: "leaf".into(),
            model: None,
            text_param: None,
            options: Vec::new(),
        },
        using: Some(qql_core::ast::VectorTarget {
            name: "dense".into(),
            kind: None,
            multi: false,
        }),
        prefetch: Vec::new(),
        mmr: None,
    };
    assert!(query_needs_kind_resolution(&query));
}

// ── New test cases for resolve_embeddings ─────────────────────────────────

#[tokio::test]
async fn a_query_text_resolved_to_dense_vector() {
    let mut stmt = Parser::parse("QUERY 'hello' FROM docs LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert_eq!(
        *input,
        QueryInput::Vector(VectorValue::Dense(vec![1.0, 2.0, 3.0]))
    );
}

#[tokio::test]
async fn b_upsert_text_resolved_to_dense_only_without_topology() {
    // Topology-unaware fallback is dense-only. Hybrid/sparse must be set by
    // the executor (schema-aware configure) or explicit USING/EMBED directives.
    let mut stmt =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hi'}, {id: 2, text: 'bye'}").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    for (i, point) in upsert.points.iter().enumerate() {
        let PointEntry::Inline(point) = point else {
            panic!("point {i} expected inline point");
        };
        let Some(PointVectors::Named(list)) = &point.vectors else {
            panic!("point {i} expected named vectors");
        };
        assert!(
            list.iter().any(|(k, v)| k == "dense"
                && matches!(v, VectorValue::Dense(d) if d == &vec![1.0, 2.0, 3.0])),
            "point {i} missing dense vector"
        );
        assert!(
            list.iter().all(|(k, _)| k != "sparse"),
            "point {i} must not receive orphan sparse vector without topology"
        );
    }
}

#[tokio::test]
async fn c_upsert_with_using_dense_model() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE MODEL 'test-model'",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter().any(|(k, v)| k == "dense"
            && matches!(v, VectorValue::Dense(d) if d == &vec![1.0, 2.0, 3.0])),
        "expected dense vector"
    );
}

#[tokio::test]
async fn d_upsert_with_embed_sparse_directive() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} EMBED text INTO sparse USING SPARSE",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter().any(|(k, v)| k == "sparse"
            && matches!(v, VectorValue::Sparse { indices, values }
                if indices == &vec![1] && values == &vec![1.0])),
        "expected sparse vector"
    );
}

#[tokio::test]
async fn e_upsert_with_using_hybrid_dense_and_sparse() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} \
         USING HYBRID DENSE MODEL 'd' SPARSE VECTOR s",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter().any(|(k, v)| k == "dense"
            && matches!(v, VectorValue::Dense(d) if d == &vec![1.0, 2.0, 3.0])),
        "expected dense vector"
    );
    assert!(
        list.iter().any(|(k, v)| k == "s"
            && matches!(v, VectorValue::Sparse { indices, values }
                if indices == &vec![1] && values == &vec![1.0])),
        "expected sparse vector"
    );
}

#[tokio::test]
async fn f1_upsert_with_embed_directive_dense() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} EMBED text INTO vec USING MODEL 'test'",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter()
            .any(|(k, v)| k == "vec"
                && matches!(v, VectorValue::Dense(d) if d == &vec![1.0, 2.0, 3.0])),
        "expected dense vector named 'vec'"
    );
}

#[tokio::test]
async fn f2_upsert_with_embed_directive_sparse() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} EMBED text INTO vec USING SPARSE",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter().any(|(k, v)| k == "vec"
            && matches!(v, VectorValue::Sparse { indices, values }
                if indices == &vec![1] && values == &vec![1.0])),
        "expected sparse vector named 'vec'"
    );
}

#[tokio::test]
async fn g_preexisting_vector_preserved_without_spec() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello', vector: {dense: [0.5, 0.5, 0.5]}}",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(
        list.iter().any(|(k, v)| k == "dense"
            && matches!(v, VectorValue::Dense(d) if d == &vec![0.5, 0.5, 0.5])),
        "pre-existing dense vector should be preserved unchanged"
    );
}

#[tokio::test]
async fn i_preprovided_query_vector_not_embedded() {
    let mut stmt = Parser::parse("QUERY NEAREST VECTOR [0.1, 0.2] FROM docs LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert_eq!(
        *input,
        QueryInput::Vector(VectorValue::Dense(vec![0.1, 0.2]))
    );
}

#[tokio::test]
async fn j_query_with_using_dense() {
    let mut stmt = Parser::parse("QUERY 'hello' FROM docs USING dense AS DENSE LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, using, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert_eq!(
        *input,
        QueryInput::Vector(VectorValue::Dense(vec![1.0, 2.0, 3.0]))
    );
    assert_eq!(
        using.as_ref().map(|target| target.name.as_str()),
        Some("dense")
    );
}

#[tokio::test]
async fn using_without_kind_fails_closed_offline() {
    let mut stmt = Parser::parse("QUERY TEXT 'x' FROM docs USING sparse LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert_eq!(err.code, "QQL-VECTOR-KIND");
    assert!(
        err.message.contains("unknown") || err.message.contains("AS DENSE|SPARSE"),
        "expected clear kind error, got: {}",
        err.message
    );
}

#[tokio::test]
async fn using_sparse_embeds_sparse_after_topology_resolution() {
    let mut stmt = Parser::parse("QUERY TEXT 'x' FROM docs USING sparse LIMIT 10").unwrap();
    let Stmt::Query(query) = &mut stmt else {
        panic!("expected Query");
    };
    resolve_query_vector_kinds(
        "docs",
        query,
        &TopologyNames {
            dense: vec!["dense".into()],
            sparse: vec!["sparse".into()],
            multivector: Vec::new(),
        },
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, using, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert!(matches!(
        input,
        QueryInput::Vector(VectorValue::Sparse { .. })
    ));
    assert_eq!(
        using.as_ref().map(|t| (t.name.as_str(), t.kind)),
        Some(("sparse", Some(qql_core::ast::VectorKind::Sparse)))
    );
}

#[tokio::test]
async fn using_as_multi_embeds_multidense() {
    let mut stmt =
        Parser::parse("QUERY TEXT 'colbert query' FROM docs USING colbert AS MULTI LIMIT 10")
            .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, using, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    match input {
        QueryInput::Vector(VectorValue::MultiDense(rows)) => {
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0], vec![0.1, 0.2]);
        }
        other => panic!("expected MultiDense, got {other:?}"),
    }
    assert!(using.as_ref().is_some_and(|t| t.multi));
    assert_eq!(mock.dense_calls.lock().unwrap().len(), 0);
    assert_eq!(mock.multi_calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn schema_multivector_name_marks_multi_and_embeds() {
    let mut stmt = Parser::parse("QUERY TEXT 'q' FROM docs USING colbert LIMIT 10").unwrap();
    let Stmt::Query(query) = &mut stmt else {
        panic!("expected Query");
    };
    resolve_query_vector_kinds(
        "docs",
        query,
        &TopologyNames {
            dense: vec!["dense".into(), "colbert".into()],
            sparse: Vec::new(),
            multivector: vec!["colbert".into()],
        },
    )
    .unwrap();
    let QueryExpr::Nearest { using, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert!(using.as_ref().is_some_and(|t| t.multi));
    assert_eq!(
        using.as_ref().and_then(|t| t.kind),
        Some(qql_core::ast::VectorKind::Dense)
    );

    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert!(matches!(
        input,
        QueryInput::Vector(VectorValue::MultiDense(_))
    ));
}

#[tokio::test]
async fn rerank_with_multivector_using_embeds_multi() {
    let mut stmt = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense AS DENSE LIMIT 100) \
         QUERY RERANK TEXT 'rerank-me' MODEL 'colbert-v2' FROM docs USING colbert AS MULTI PREFETCH (c) LIMIT 10;",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();
    let multi = mock.multi_calls.lock().unwrap();
    assert!(
        multi
            .iter()
            .any(|(m, t)| m == "colbert-v2" && t == "rerank-me"),
        "expected multi embed with colbert model, got: {:?}",
        *multi
    );
}

#[tokio::test]
async fn query_with_arbitrary_sparse_vector_name_uses_sparse_embedding() {
    let mut stmt =
        Parser::parse("QUERY 'hello' FROM docs USING lexical_v2 AS SPARSE LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest { input, using, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    assert!(matches!(
        input,
        QueryInput::Vector(VectorValue::Sparse { .. })
    ));
    assert_eq!(
        using.as_ref().map(|target| target.name.as_str()),
        Some("lexical_v2")
    );
}

#[tokio::test]
async fn test_deterministic_field_priority() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'title text', text: 'primary text'} USING DENSE MODEL 'test-model'",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let calls = mock.dense_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "primary text");
}

#[tokio::test]
async fn test_on_field_explicit_resolution() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'primary text', title: 'title text'} USING DENSE MODEL 'test-model' ON FIELD title INTO title_vec",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let calls = mock.dense_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "title text");

    let Stmt::Upsert(upsert) = &stmt else {
        panic!("expected Upsert");
    };
    let Some(PointVectors::Named(list)) =
        &upsert.points[0].as_inline().expect("inline point").vectors
    else {
        panic!("expected named vectors");
    };
    assert!(list.iter().any(|(k, _)| k == "title_vec"));
}

#[tokio::test]
async fn test_on_field_missing_errors_loudly() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'primary text'} USING DENSE MODEL 'test-model' ON FIELD missing_field",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(err.message.contains("ON FIELD 'missing_field'"));
}

#[tokio::test]
async fn test_no_text_field_errors_loudly() {
    let mut stmt =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, score: 99} USING DENSE MODEL 'test-model'")
            .unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(err.message.contains(
        "Expected one of: text, body, content, title, description, name, summary, document"
    ));
}

#[tokio::test]
async fn test_multi_spec_field_resolution() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'header', body: 'content text'} USING DENSE MODEL 'm1' ON FIELD title INTO t_vec, DENSE MODEL 'm2' ON FIELD body INTO b_vec",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let calls = mock.dense_calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, "header");
    assert_eq!(calls[1].1, "content text");
}

#[tokio::test]
async fn test_duplicate_target_vector_error() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE MODEL 'm1' INTO vec1, DENSE MODEL 'm2' INTO vec1",
    )
    .unwrap();
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(err.message.contains("duplicate target vector 'vec1'"));
}

// ── Embedder trait default-behaviour tests ───────────────────────────────

/// Minimal embedder that only provides `embed_dense`; all other methods
/// use their trait defaults (no override).
struct DefaultEmbedder;

#[async_trait]
impl Embedder for DefaultEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(vec![0.0])
    }
}

#[tokio::test]
async fn default_embed_sparse_rejects_non_default_model() {
    let e = DefaultEmbedder;
    // Empty / default is accepted → returns local wire-compatible BM25.
    let sv = e.embed_sparse_query("hello", "").await.unwrap();
    assert!(!sv.indices.is_empty());
    let sv = e.embed_sparse_document("hello", "default").await.unwrap();
    assert!(!sv.indices.is_empty());
    // Non-default model is rejected.
    let err = e.embed_sparse_query("hello", "splade").await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING-SPARSE");
    assert!(err.message.contains("splade"));
}

#[tokio::test]
async fn default_sparse_methods_match_the_shared_pipeline() {
    // The default trait methods reuse the process-wide pipeline when the
    // configured text options are the defaults; output must stay
    // byte-identical to the free-function pipeline wrappers.
    let e = DefaultEmbedder;
    for text in [
        "Recipe for baking chocolate chip cookies",
        "the and of to in",
        "",
    ] {
        assert_eq!(
            e.embed_sparse_document(text, "default").await.unwrap(),
            crate::sparse::embed_document(text),
            "document output changed for {text:?}"
        );
        assert_eq!(
            e.embed_sparse_query(text, "").await.unwrap(),
            crate::sparse::embed_query(text),
            "query output changed for {text:?}"
        );
        assert_eq!(
            e.embed_sparse_document(text, "QDRANT/BM25").await.unwrap(),
            crate::sparse::embed_document(text),
            "alias output changed for {text:?}"
        );
    }
}

/// Default sparse methods with a non-default text config (Spanish), so the
/// per-config pipeline branch is exercised instead of the cached default.
struct SpanishEmbedder;

#[async_trait]
impl Embedder for SpanishEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(vec![0.0])
    }

    fn bm25_text_config(&self) -> crate::Bm25TextConfig {
        crate::Bm25TextConfig {
            language: crate::Language::Spanish,
            ..crate::Bm25TextConfig::default()
        }
    }
}

#[tokio::test]
async fn overridden_bm25_text_config_is_honored_by_default_sparse_methods() {
    let e = SpanishEmbedder;
    let expected = e
        .bm25_text_config()
        .pipeline()
        .embed_document("la casa")
        .unwrap();
    let got = e.embed_sparse_document("la casa", "default").await.unwrap();
    assert_eq!(got, expected);
    assert_ne!(
        got,
        crate::sparse::embed_document("la casa"),
        "the Spanish override must not fall back to the English default"
    );
    assert_eq!(
        e.embed_sparse_query("la casa", "qdrant/bm25")
            .await
            .unwrap(),
        e.bm25_text_config()
            .pipeline()
            .embed_query("la casa")
            .unwrap()
    );
}

#[tokio::test]
async fn default_embed_sparse_accepts_qdrant_bm25_alias() {
    let e = DefaultEmbedder;
    // The local pipeline *is* `qdrant/bm25`; the server spelling must be
    // accepted (any case) instead of rejected.
    for alias in ["qdrant/bm25", "QDRANT/BM25"] {
        let query = e.embed_sparse_query("hello world", alias).await.unwrap();
        let document = e.embed_sparse_document("hello world", alias).await.unwrap();
        assert_eq!(
            query,
            e.embed_sparse_query("hello world", "default")
                .await
                .unwrap()
        );
        assert_eq!(
            document,
            e.embed_sparse_document("hello world", "").await.unwrap()
        );
    }
    // Unrelated models still fail closed with the alias in the message.
    let err = e.embed_sparse_query("hello", "splade").await.unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING-SPARSE");
    assert!(err.message.contains("qdrant/bm25"), "got: {}", err.message);
}

/// Embedder echoing the first text byte as the vector, so per-model batch
/// regrouping can be checked for walk-order restoration.
struct OrderMockEmbedder;

fn first_byte_vec(text: &str) -> Vec<f32> {
    vec![text.bytes().next().unwrap_or(0) as f32]
}

#[async_trait]
impl Embedder for OrderMockEmbedder {
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(first_byte_vec(text))
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        _model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        Ok(texts.iter().map(|t| first_byte_vec(t)).collect())
    }
}

fn dense_input_of(query: &QueryStmt) -> Vec<f32> {
    let QueryExpr::Nearest { input, .. } = &query.expression else {
        panic!("expected Nearest");
    };
    let QueryInput::Vector(VectorValue::Dense(vec)) = input else {
        panic!("expected resolved dense vector, got {input:?}");
    };
    vec.clone()
}

#[tokio::test]
async fn multi_model_batch_restores_walk_order() {
    // Interleaved models (m1, m2, m1) batch per model but must apply in walk
    // order: alpha → beta → gamma.
    let mut stmt = Parser::parse(
        "WITH a AS (QUERY TEXT 'alpha' MODEL 'm1' USING dense AS DENSE LIMIT 10), \
         b AS (QUERY TEXT 'beta' MODEL 'm2' USING dense AS DENSE PREFETCH (a) LIMIT 10) \
         QUERY TEXT 'gamma' MODEL 'm1' FROM docs USING dense AS DENSE PREFETCH (b) LIMIT 10;",
    )
    .unwrap();
    resolve_embeddings(&mut stmt, &OrderMockEmbedder)
        .await
        .unwrap();
    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    assert_eq!(query.ctes.len(), 2);
    let first = dense_input_of(&query.ctes[0].query);
    let second = dense_input_of(&query.ctes[1].query);
    assert_eq!(first, vec![b'a' as f32]);
    assert_eq!(second, vec![b'b' as f32]);
    assert_eq!(dense_input_of(query), vec![b'g' as f32]);
}

#[tokio::test]
async fn cte_prefetch_reference_resolves_to_the_embedded_body() {
    // Regression: `PREFETCH (a)` inside CTE `b` used to be resolved by the
    // planner against a parse-time clone of `a` that the embedding walk never
    // visited, so the stage planned as a server-side `Document` instead of
    // the client-embedded vector. The body is now stored once and both walks
    // (embed, plan) see the same embedded AST.
    let mut stmt = Parser::parse(
        "WITH a AS (QUERY TEXT 'alpha' FROM docs USING dense AS DENSE LIMIT 5), \
         b AS (QUERY TEXT 'beta' FROM docs USING dense AS DENSE PREFETCH (a) LIMIT 5) \
         QUERY TEXT 'gamma' FROM docs USING dense AS DENSE PREFETCH (b) LIMIT 5;",
    )
    .unwrap();
    resolve_embeddings(&mut stmt, &OrderMockEmbedder)
        .await
        .unwrap();

    let op = qql_plan::plan(&stmt).expect("embedded query must plan");
    let qql_plan::PlannedOperation::Query { request, .. } = op else {
        panic!("expected Query, got {op:?}");
    };
    let b_stage = request.prefetch[0]
        .query
        .as_ref()
        .expect("outer PREFETCH (b) stage");
    let a_stage = request.prefetch[0]
        .prefetch
        .as_ref()
        .expect("b's nested PREFETCH list")[0]
        .query
        .as_ref()
        .expect("nested PREFETCH (a) stage");
    for (label, variant) in [("b", b_stage), ("a", a_stage)] {
        let qql_plan::QueryVariant::Nearest(nearest) = variant else {
            panic!("{label} stage must be a nearest query: {variant:?}");
        };
        match &nearest.nearest {
            qql_plan::PlanQueryInput::Vector(qql_plan::PlanVectorValue::Dense(vec)) => {
                assert_eq!(vec, &first_byte_vec(label));
            }
            other => panic!(
                "{label} stage must carry the client-embedded vector, got {other:?} \
                 (a Document here means the embedding walk missed the CTE body)"
            ),
        }
    }
}

#[tokio::test]
async fn default_batch_loops_cover_single_methods() {
    let e = DefaultEmbedder;
    // Dense batch loops `embed_dense`.
    let vecs = e
        .embed_dense_batch(&["a".to_string(), "b".to_string()], "m")
        .await
        .unwrap();
    assert_eq!(vecs, vec![vec![0.0], vec![0.0]]);
    // Sparse query batch loops `embed_sparse_query` (default model → BM25).
    let batch = e
        .embed_sparse_query_batch(&["hello world".to_string(), "hello".to_string()], "default")
        .await
        .unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(
        batch[0],
        e.embed_sparse_query("hello world", "default")
            .await
            .unwrap()
    );
    // Sparse document batch loops `embed_sparse_document`.
    let docs = e
        .embed_sparse_document_batch(&["hello world".to_string()], "")
        .await
        .unwrap();
    assert_eq!(docs.len(), 1);
    // The non-default reject gate holds through the batch entry too.
    let err = e
        .embed_sparse_query_batch(&["x".to_string()], "splade")
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EMBEDDING-SPARSE");
}

#[tokio::test]
async fn default_opt_in_modalities_reject_with_codes() {
    let e = DefaultEmbedder;
    assert_eq!(
        e.embed_multi("x", "m").await.unwrap_err().code,
        "QQL-EMBEDDING-MULTI"
    );
    assert_eq!(
        e.embed_image("p", "m").await.unwrap_err().code,
        "QQL-EMBEDDING-IMAGE"
    );
    assert_eq!(
        e.rerank_pairs("q", &["d".to_string()], "m")
            .await
            .unwrap_err()
            .code,
        "QQL-RERANK-CROSS"
    );
    // Batch variants loop the rejecting singles → same codes.
    assert_eq!(
        e.embed_multi_batch(&["x".to_string()], "m")
            .await
            .unwrap_err()
            .code,
        "QQL-EMBEDDING-MULTI"
    );
    assert_eq!(
        e.embed_image_batch(&["p".to_string()], "m")
            .await
            .unwrap_err()
            .code,
        "QQL-EMBEDDING-IMAGE"
    );
}

#[tokio::test]
async fn default_dimension_and_model_checks() {
    let e = DefaultEmbedder;
    assert_eq!(e.dimension(), None);
    assert_eq!(e.multi_dimension(), None);
    assert!(e.accepts_model("anything"));
}

#[tokio::test]
async fn dense_model_unsupported_error_shape() {
    let err = crate::embedder::dense_model_unsupported_error("nomic");
    assert_eq!(err.code, "QQL-EMBEDDING");
    assert!(err.message.contains("nomic"), "got: {}", err.message);
}
