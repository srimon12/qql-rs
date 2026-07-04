use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use qql_core::ast;
use qql_core::error::QqlError;

use crate::embedder::Embedder;
use crate::pipeline;
use crate::pipeline::*;
use crate::sparse;

// ── helpers ──────────────────────────────────────────────────────

struct CallCounterNode {
    count: Arc<AtomicBool>,
    fail: bool,
}

#[async_trait]
impl ExecutionNode for CallCounterNode {
    async fn execute(&self, _state: &mut QueryState) -> Result<(), QqlError> {
        self.count.store(true, Ordering::SeqCst);
        if self.fail {
            Err(QqlError::runtime("node 1 failed"))
        } else {
            Ok(())
        }
    }
}

struct MockEmbedder {
    dense: Vec<f32>,
    sparse_indices: Vec<u32>,
    sparse_values: Vec<f32>,
    err: Option<QqlError>,
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        if let Some(ref e) = self.err {
            return Err(e.clone());
        }
        Ok(self.dense.clone())
    }
    async fn embed_sparse(&self, _text: &str) -> Result<sparse::SparseVector, QqlError> {
        if let Some(ref e) = self.err {
            return Err(e.clone());
        }
        Ok(sparse::SparseVector {
            indices: self.sparse_indices.clone(),
            values: self.sparse_values.clone(),
        })
    }
}

// ── Pipeline tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_query_pipeline_execute_success() {
    let mut p = QueryPipeline::new();
    let mut state = QueryState {
        query_text: "test".to_string(),
        ..Default::default()
    };

    let executed1 = Arc::new(AtomicBool::new(false));
    let executed2 = Arc::new(AtomicBool::new(false));

    p.add(Box::new(CallCounterNode {
        count: executed1.clone(),
        fail: false,
    }));
    p.add(Box::new(CallCounterNode {
        count: executed2.clone(),
        fail: false,
    }));

    let result = p.execute(&mut state).await;
    assert!(result.is_ok());
    assert!(executed1.load(Ordering::SeqCst));
    assert!(executed2.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_query_pipeline_execute_error_stops_execution() {
    let mut p = QueryPipeline::new();
    let mut state = QueryState {
        query_text: "test".to_string(),
        ..Default::default()
    };

    let executed1 = Arc::new(AtomicBool::new(false));
    let executed2 = Arc::new(AtomicBool::new(false));

    p.add(Box::new(CallCounterNode {
        count: executed1.clone(),
        fail: true,
    }));
    p.add(Box::new(CallCounterNode {
        count: executed2.clone(),
        fail: false,
    }));

    let result = p.execute(&mut state).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().msg.as_ref(), "node 1 failed");
    assert!(executed1.load(Ordering::SeqCst));
    assert!(!executed2.load(Ordering::SeqCst));
}

#[test]
fn test_build_flat_request_sets_all_fields() {
    let p = QueryPipeline::new();
    let state = QueryState {
        collection_name: "docs".to_string(),
        vector_name: "dense".to_string(),
        limit: 10,
        offset: 5,
        score_threshold: Some(0.5),
        request_timeout: Some(30),
        ..Default::default()
    };

    let req = p.build_flat_request(&state).unwrap();

    assert_eq!(req.collection_name, "docs");
    assert_eq!(req.limit, 10);
    assert_eq!(req.offset, 5);
    assert_eq!(req.using, Some("dense".to_string()));
    assert!((req.score_threshold.unwrap() - 0.5).abs() < 1e-6);
    assert_eq!(req.timeout, Some(30));
}

#[test]
fn test_build_flat_request_omits_zero_offset() {
    let p = QueryPipeline::new();
    let state = QueryState {
        collection_name: "docs".to_string(),
        limit: 10,
        offset: 0,
        ..Default::default()
    };

    let req = p.build_flat_request(&state).unwrap();
    assert_eq!(req.offset, 0);
}

#[test]
fn test_build_grouped_request_inherits_flat_fields() {
    let p = QueryPipeline::new();
    let state = QueryState {
        collection_name: "docs".to_string(),
        limit: 10,
        group_by: "category".to_string(),
        group_size: 3,
        ..Default::default()
    };

    let req = p.build_grouped_request(&state).unwrap();

    assert_eq!(req.collection_name, "docs");
    assert_eq!(req.limit, 10);
    assert_eq!(req.group_by, "category");
    assert_eq!(req.group_size, 3);
}

#[test]
fn test_get_doc_options_caches_result() {
    let mut state = QueryState {
        cloud_model_options: {
            let mut m = HashMap::new();
            m.insert("key".to_string(), "val".to_string());
            m
        },
        ..Default::default()
    };

    let opts1 = state.get_doc_options();
    let opts2 = state.get_doc_options();

    assert_eq!(opts1.get("key").unwrap(), "val");
    assert_eq!(opts1, opts2);
}

#[test]
fn test_get_doc_options_nil_for_empty_config() {
    let mut state = QueryState::default();
    let opts = state.get_doc_options();
    assert!(opts.is_empty());
}

#[test]
fn test_build_expression_match_condition() {
    let expr = ast::FormulaExpr::MatchCondition {
        field: "tag",
        values: vec![
            ast::Value::Str(std::borrow::Cow::Borrowed("h1")),
            ast::Value::Str(std::borrow::Cow::Borrowed("h2")),
            ast::Value::Str(std::borrow::Cow::Borrowed("h3")),
        ],
    };

    let result = pipeline::build_expression(&expr);
    assert!(result.is_ok());

    let json = result.unwrap();
    let condition = json.get("condition").unwrap();
    let field_cond = condition.get("match").unwrap();
    assert_eq!(field_cond.get("key").unwrap().as_str().unwrap(), "tag");
    let values = field_cond.get("values").unwrap().as_array().unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn test_build_expression_match_condition_single() {
    let expr = ast::FormulaExpr::MatchCondition {
        field: "category",
        values: vec![ast::Value::Str(std::borrow::Cow::Borrowed("premium"))],
    };

    let result = pipeline::build_expression(&expr);
    assert!(result.is_ok());

    let json = result.unwrap();
    let condition = json.get("condition").unwrap();
    let field_cond = condition.get("match").unwrap();
    assert_eq!(field_cond.get("key").unwrap().as_str().unwrap(), "category");
    assert_eq!(
        field_cond
            .get("value")
            .unwrap()
            .get("str")
            .unwrap()
            .as_str()
            .unwrap(),
        "premium"
    );
}

#[test]
fn test_build_expression_match_condition_numeric() {
    let expr = ast::FormulaExpr::MatchCondition {
        field: "count",
        values: vec![ast::Value::Int(1), ast::Value::Int(2), ast::Value::Int(3)],
    };

    let result = pipeline::build_expression(&expr);
    assert!(result.is_ok());

    let json = result.unwrap();
    let condition = json.get("condition").unwrap();
    let field_cond = condition.get("match").unwrap();
    assert_eq!(field_cond.get("key").unwrap().as_str().unwrap(), "count");
    let values = field_cond.get("values").unwrap().as_array().unwrap();
    let ints: Vec<i64> = values
        .iter()
        .map(|v| v.get("int").unwrap().as_i64().unwrap())
        .collect();
    assert_eq!(ints, vec![1, 2, 3]);
}

// ── Node tests ───────────────────────────────────────────────────

#[tokio::test]
async fn test_dense_embed_node_execute_cloud() {
    let node = DenseEmbedNode {
        model: "test-model".to_string(),
        vector_name: "dense".to_string(),
        limit: 10,
        as_prefetch: false,
    };

    let mut state = QueryState {
        query_text: "hello".to_string(),
        local_embed: false,
        ..Default::default()
    };

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Document { text, model, .. } => {
            assert_eq!(text, "hello");
            assert_eq!(model, "test-model");
        }
        _ => panic!("expected Document variant"),
    }
}

#[tokio::test]
async fn test_dense_embed_node_execute_local() {
    let node = DenseEmbedNode {
        model: "test-model".to_string(),
        vector_name: "dense".to_string(),
        limit: 10,
        as_prefetch: false,
    };

    let mut state = QueryState {
        query_text: "hello".to_string(),
        local_embed: true,
        embedder: Some(Arc::new(MockEmbedder {
            dense: vec![0.1, 0.2, 0.3],
            sparse_indices: Vec::new(),
            sparse_values: Vec::new(),
            err: None,
        })),
        ..Default::default()
    };

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Nearest(v) => {
            assert_eq!(v, &vec![0.1, 0.2, 0.3]);
        }
        _ => panic!("expected Nearest variant"),
    }
}

#[tokio::test]
async fn test_sparse_embed_node_execute_cloud() {
    let node = SparseEmbedNode {
        model: "test-sparse-model".to_string(),
        vector_name: "sparse".to_string(),
        limit: 10,
        as_prefetch: false,
    };

    let mut state = QueryState {
        query_text: "hello".to_string(),
        local_embed: false,
        ..Default::default()
    };

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Document { text, model, .. } => {
            assert_eq!(text, "hello");
            assert_eq!(model, "test-sparse-model");
        }
        _ => panic!("expected Document variant"),
    }
}

#[tokio::test]
async fn test_sparse_embed_node_execute_local() {
    let node = SparseEmbedNode {
        model: "test-sparse-model".to_string(),
        vector_name: "sparse".to_string(),
        limit: 10,
        as_prefetch: false,
    };

    let mut state = QueryState {
        query_text: "hello".to_string(),
        local_embed: true,
        embedder: Some(Arc::new(MockEmbedder {
            dense: Vec::new(),
            sparse_indices: vec![1, 5, 9],
            sparse_values: vec![0.1, 0.5, 0.9],
            err: None,
        })),
        ..Default::default()
    };

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Sparse(indices, values) => {
            assert_eq!(indices, &vec![1, 5, 9]);
            assert_eq!(values, &vec![0.1, 0.5, 0.9]);
        }
        _ => panic!("expected Sparse variant"),
    }
}

#[tokio::test]
async fn test_fusion_node_execute() {
    let node = FusionNode {
        mode: "rrf".to_string(),
    };
    let mut state = QueryState::default();

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());
    assert_eq!(
        state.target_query,
        Some(QueryVariant::Fusion(FusionType::Rrf))
    );

    let node2 = FusionNode {
        mode: "dbsf".to_string(),
    };
    let mut state2 = QueryState::default();
    let result = node2.execute(&mut state2).await;
    assert!(result.is_ok());
    assert_eq!(
        state2.target_query,
        Some(QueryVariant::Fusion(FusionType::Dbsf))
    );
}

#[tokio::test]
async fn test_rerank_node_execute() {
    let node = RerankNode {
        model: "rerank-model".to_string(),
    };
    let mut state = QueryState {
        query_text: "hello".to_string(),
        local_embed: false,
        ..Default::default()
    };

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Document { text, model, .. } => {
            assert_eq!(text, "hello");
            assert_eq!(model, "rerank-model");
        }
        _ => panic!("expected Document variant"),
    }
}

#[tokio::test]
async fn test_recommend_node_execute() {
    let node = RecommendNode {
        positive_ids: vec![
            ast::Value::Str(std::borrow::Cow::Borrowed(
                "123e4567-e89b-12d3-a456-426614174000",
            )),
            ast::Value::Int(42),
        ],
        negative_ids: vec![ast::Value::Str(std::borrow::Cow::Borrowed(
            "123e4567-e89b-12d3-a456-426614174001",
        ))],
        strategy: None,
    };
    let mut state = QueryState::default();

    let result = node.execute(&mut state).await;
    assert!(result.is_ok());

    let query = state.target_query.unwrap();
    match &query {
        QueryVariant::Recommend(rec) => {
            assert_eq!(rec.positive.len(), 2);
            assert_eq!(rec.negative.len(), 1);
            match &rec.positive[0] {
                VectorInput::Id(PointId::Uuid(u)) => {
                    assert_eq!(u, "123e4567-e89b-12d3-a456-426614174000");
                }
                _ => panic!("expected UUID point id"),
            }
            match &rec.positive[1] {
                VectorInput::Id(PointId::Num(n)) => {
                    assert_eq!(*n, 42);
                }
                _ => panic!("expected Num point id"),
            }
            match &rec.negative[0] {
                VectorInput::Id(PointId::Uuid(u)) => {
                    assert_eq!(u, "123e4567-e89b-12d3-a456-426614174001");
                }
                _ => panic!("expected UUID point id"),
            }
        }
        _ => panic!("expected Recommend variant"),
    }
}

// ── Additional utility tests ─────────────────────────────────────

#[test]
fn test_search_params_default() {
    let with = ast::SearchWith {
        hnsw_ef: 0,
        exact: false,
        acorn: false,
        indexed_only: false,
        quantization: None,
        mmr_diversity: None,
        mmr_candidates: None,
        rrf_k: None,
        rrf_weights: Vec::new(),
    };
    let params = pipeline::build_search_params(&with);
    assert!(params.is_none());
}

#[test]
fn test_search_params_with_values() {
    let with = ast::SearchWith {
        hnsw_ef: 256,
        exact: true,
        acorn: false,
        indexed_only: false,
        quantization: None,
        mmr_diversity: None,
        mmr_candidates: None,
        rrf_k: None,
        rrf_weights: Vec::new(),
    };
    let params = pipeline::build_search_params(&with);
    assert!(params.is_some());
    let p = params.unwrap();
    assert_eq!(p.hnsw_ef, Some(256));
    assert_eq!(p.exact, Some(true));
}

#[test]
fn test_point_id_num() {
    let id = pipeline::to_point_id(&ast::Value::Int(42)).unwrap();
    assert_eq!(id, PointId::Num(42));
}

#[test]
fn test_point_id_uuid() {
    let id = pipeline::to_point_id(&ast::Value::Str(std::borrow::Cow::Borrowed(
        "550e8400-e29b-41d4-a716-446655440000",
    )))
    .unwrap();
    match id {
        PointId::Uuid(_) => {}
        _ => panic!("expected UUID"),
    }
}

#[test]
fn test_has_mmr() {
    let with = ast::SearchWith::default();
    let has = with.mmr_diversity.is_some() && with.mmr_candidates.is_some();
    assert!(!has);
}

#[test]
fn test_mmr_enabled() {
    let with = ast::SearchWith {
        mmr_diversity: Some(0.5),
        mmr_candidates: Some(10),
        ..Default::default()
    };
    let has = with.mmr_diversity.is_some() && with.mmr_candidates.is_some();
    assert!(has);
}

#[test]
fn test_query_variant_serialization_roundtrip() {
    use crate::pipeline::QueryVariant;

    let variant = QueryVariant::Nearest(vec![1.0, 2.0, 3.0]);
    let serialized = serde_json::to_value(&variant).unwrap();
    let expected = serde_json::json!({"nearest": [1.0, 2.0, 3.0]});
    assert_eq!(serialized, expected);
    let deserialized: QueryVariant = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, variant);
}
