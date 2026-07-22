//! Focused regression tests for embedding resolution (EMBED-001..005).

use async_trait::async_trait;
use qql_core::ast::{PointId, PointVectors, Stmt, UpsertPoint, UpsertStmt, VectorValue};
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
        calls.iter().any(|(m, t)| m == "colbert-v2" && t == "rerank-me"),
        "expected model=colbert-v2 for rerank text, got: {:?}",
        *calls
    );
    assert!(
        !calls.iter().any(|(m, t)| m == "colbert" && t == "rerank-me"),
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
