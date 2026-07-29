//! Focused regression tests for embedding resolution (EMBED-001..005).

use async_trait::async_trait;
use qql_core::ast::{
    PointId, PointVectors, QueryExpr, QueryInput, Stmt, UpsertPoint, UpsertStmt, VectorValue,
};
use qql_core::error::QqlError;
use qql_core::parser::Parser;
use std::sync::{Arc, Mutex};

use crate::embedder::Embedder;
use crate::resolve::resolve_embeddings;
use crate::sparse::SparseVector;
use crate::topology::{resolve_query_vector_kinds, TopologyNames};

struct MockEmbedder {
    dense_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, text)
    multi_calls: Arc<Mutex<Vec<(String, String)>>>,
    image_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, source)
    dense_batch_override: Option<Vec<Vec<f32>>>,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self {
            dense_calls: Arc::new(Mutex::new(Vec::new())),
            multi_calls: Arc::new(Mutex::new(Vec::new())),
            image_calls: Arc::new(Mutex::new(Vec::new())),
            dense_batch_override: None,
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

    async fn embed_sparse(&self, _text: &str) -> Result<SparseVector, QqlError> {
        Ok(SparseVector {
            indices: vec![1],
            values: vec![1.0],
        })
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
            UpsertPoint {
                id: PointId::Number(1),
                vectors: None,
                payload: vec![("text".into(), qql_core::ast::Value::Str("a".into()))],
            },
            UpsertPoint {
                id: PointId::Number(2),
                vectors: None,
                payload: vec![("text".into(), qql_core::ast::Value::Str("b".into()))],
            },
        ],
        embedding: Some(qql_core::ast::EmbeddingSpec::Dense {
            model: Some("m".into()),
            vector: Some("dense".into()),
            field: None,
        }),
        embed: vec![],
        shard_key: None,
    }));
    let mock = MockEmbedder {
        dense_batch_override: Some(vec![vec![1.0]]), // only 1 vector for 2 texts
        ..Default::default()
    };
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(
        err.message.contains("embed_dense_batch") || err.code.contains("EMBEDDING"),
        "expected cardinality error, got: {}",
        err
    );
}

#[tokio::test]
async fn unnamed_vector_topology_conflict_rejected() {
    let mut stmt = Stmt::Upsert(Box::new(UpsertStmt {
        collection: "docs".into(),
        points: vec![UpsertPoint {
            id: PointId::Number(1),
            vectors: Some(PointVectors::Unnamed(VectorValue::Dense(vec![0.1, 0.2]))),
            payload: vec![("text".into(), qql_core::ast::Value::Str("hello".into()))],
        }],
        embedding: Some(qql_core::ast::EmbeddingSpec::Dense {
            model: Some("m".into()),
            vector: Some("dense".into()),
            field: None,
        }),
        embed: vec![],
        shard_key: None,
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
    match &u.points[0].vectors {
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
    match &u.points[0].vectors {
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
async fn sparse_model_is_rejected() {
    let mut stmt = Stmt::Upsert(Box::new(UpsertStmt {
        collection: "docs".into(),
        points: vec![UpsertPoint {
            id: PointId::Number(1),
            vectors: None,
            payload: vec![("text".into(), qql_core::ast::Value::Str("hello".into()))],
        }],
        embedding: None,
        embed: vec![qql_core::ast::EmbedDirective {
            source_field: "text".into(),
            target_vector: "sparse".into(),
            kind: qql_core::ast::EmbedKind::Sparse {
                model: Some("other".into()),
            },
        }],
        shard_key: None,
    }));
    let mock = MockEmbedder::default();
    let err = resolve_embeddings(&mut stmt, &mock).await.unwrap_err();
    assert!(
        err.message.to_ascii_lowercase().contains("sparse model"),
        "expected sparse model rejection, got: {}",
        err
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
    let Some(PointVectors::Named(list)) = &upsert.points[0].vectors else {
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
