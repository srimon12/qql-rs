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

#[derive(Default)]
struct MockEmbedder {
    dense_calls: Arc<Mutex<Vec<(String, String)>>>, // (model, text)
    dense_batch_override: Option<Vec<Vec<f32>>>,
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
}

#[tokio::test]
async fn rerank_uses_model_not_vector_name() {
    let mut stmt = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense LIMIT 100) \
         QUERY RERANK TEXT 'rerank-me' MODEL 'colbert-v2' FROM docs USING colbert PREFETCH (c) LIMIT 10;",
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
        "WITH a AS (QUERY TEXT 'first' USING dense LIMIT 10), \
         b AS (QUERY TEXT 'second' USING dense PREFETCH (a) LIMIT 10) \
         QUERY TEXT 'third' FROM docs USING dense PREFETCH (b) LIMIT 10;",
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
async fn b_upsert_text_resolved_to_dense_and_sparse() {
    let mut stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hi'}, {id: 2, text: 'bye'}",
    )
    .unwrap();
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
            list.iter().any(|(k, v)| k == "sparse"
                && matches!(v, VectorValue::Sparse { indices, values }
                    if indices == &vec![1] && values == &vec![1.0])),
            "point {i} missing sparse vector"
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
        list.iter().any(|(k, v)| k == "vec"
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
    let mut stmt = Parser::parse(
        "QUERY NEAREST VECTOR [0.1, 0.2] FROM docs LIMIT 10",
    )
    .unwrap();
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
    let mut stmt =
        Parser::parse("QUERY 'hello' FROM docs USING dense LIMIT 10").unwrap();
    let mock = MockEmbedder::default();
    resolve_embeddings(&mut stmt, &mock).await.unwrap();

    let Stmt::Query(query) = &stmt else {
        panic!("expected Query");
    };
    let QueryExpr::Nearest {
        input, using, ..
    } = &query.expression
    else {
        panic!("expected Nearest");
    };
    assert_eq!(
        *input,
        QueryInput::Vector(VectorValue::Dense(vec![1.0, 2.0, 3.0]))
    );
    assert_eq!(using.as_deref(), Some("dense"));
}
