use serde_json;
use std::collections::HashMap;

use crate::executor::{ExecResponse, Executor, SearchHit};
use crate::filter_conv::FilterConverter;
use crate::pipeline::{
    self, DenseEmbedNode, DiscoverNode, FusionNode, OrderByNode, QueryPipeline, QueryState,
    RawVectorNode, RecommendNode, RelevanceFeedbackNode, RerankNode, SampleNode, SparseEmbedNode,
    WithPayload,
};
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

use crate::executor::helpers::{
    build_with_payload, build_with_vectors, clone_value, has_mmr, point_id_string,
};

impl Executor {
    pub(crate) async fn build_query_state_and_pipeline(
        &self,
        stmt: &ast::QueryStmt<'_>,
    ) -> Result<(QueryState, QueryPipeline), QqlError> {
        let dense_vector_name: String;
        let sparse_vector_name: String;

        if let Some(using) = stmt.using_ {
            dense_vector_name = using.to_string();
            sparse_vector_name = using.to_string();
        } else {
            let topo = self
                .resolve_vector_topology(stmt.collection.unwrap_or(""))
                .await?;
            if let Some(ref dv) = topo.dense_vector {
                if !dv.is_empty() {
                    dense_vector_name = dv.clone();
                    if let Some(ref sv) = topo.sparse_vector {
                        if !sv.is_empty() {
                            sparse_vector_name = sv.clone();
                        } else {
                            sparse_vector_name = dense_vector_name.clone();
                        }
                    } else {
                        sparse_vector_name = dense_vector_name.clone();
                    }
                } else {
                    dense_vector_name = String::new();
                    sparse_vector_name = String::new();
                }
            } else {
                dense_vector_name = String::new();
                sparse_vector_name = String::new();
            }
        }

        let dense_model = self.resolve_dense_model(stmt.model);
        let sparse_model = if stmt.query_type == ast::QueryType::Hybrid {
            Some(self.resolve_sparse_model(stmt.model))
        } else {
            None
        };

        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(filter)?
        } else {
            None
        };

        let mut state = QueryState {
            query_text: stmt.query_text.map(|s| s.to_string()).unwrap_or_default(),
            prefetches: Vec::new(),
            manual_prefetches: Vec::new(),
            target_query: None,
            params: stmt
                .with_clause
                .as_ref()
                .and_then(|wc| pipeline::build_search_params(wc)),
            fusion_config: None,
            has_mmr: has_mmr(stmt.with_clause.as_ref().map(|wc| wc.as_ref())),
            mmr_candidates: 0,
            mmr_diversity: 0.0,
            local_embed: self.uses_local_embeddings(),
            embedder: self.embedder.clone(),
            cloud_model_options: self.cloud_model_options(),
            dense_model: dense_model.clone(),

            doc_options: None,
            request_timeout: self.request_timeout(),

            collection_name: stmt.collection.map(|s| s.to_string()).unwrap_or_default(),
            vector_name: String::new(),
            limit: stmt.limit as u64,
            offset: stmt.offset as u64,
            qdrant_filter,
            score_threshold: stmt.score_threshold.map(|v| v as f32),
            lookup_from: if !stmt.lookup_from.unwrap_or("").is_empty() {
                Some(pipeline::LookupLocation {
                    collection_name: stmt.lookup_from.unwrap().to_string(),
                    vector_name: stmt.lookup_vector.map(|s| s.to_string()),
                })
            } else {
                None
            },
            with_payload: build_with_payload(stmt.with_payload.as_ref().map(|p| p.as_ref())),
            with_vectors: build_with_vectors(stmt.with_vectors.as_ref().map(|v| v.as_ref())),
            group_by: stmt.group_by.map(|s| s.to_string()).unwrap_or_default(),
            group_size: stmt.group_size.unwrap_or(0) as u64,
            with_lookup: stmt.with_lookup_collection.map(|c| pipeline::WithLookup {
                collection: c.to_string(),
            }),
            formula: None,
            formula_defaults: HashMap::new(),
        };

        // Fusion config
        if let Some(ref wc) = stmt.with_clause {
            if wc.rrf_k.is_some() || !wc.rrf_weights.is_empty() {
                state.fusion_config = Some(pipeline::RrfConfig {
                    k: wc.rrf_k.map(|k| k as u32),
                    weights: wc.rrf_weights.clone(),
                });
            }
        }

        // MMR
        if state.has_mmr {
            if let Some(ref wc) = stmt.with_clause {
                if let Some(d) = wc.mmr_diversity {
                    state.mmr_diversity = d as f32;
                }
                if let Some(c) = wc.mmr_candidates {
                    state.mmr_candidates = c as u32;
                }
            }
        }

        let mut exec_pipeline = QueryPipeline::new();

        match stmt.mode {
            ast::QueryMode::OrderBy => {
                let asc = stmt.order_by_asc.unwrap_or(true);
                exec_pipeline.add(Box::new(OrderByNode {
                    field: stmt
                        .order_by_field
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    asc,
                }));
            }
            ast::QueryMode::Sample => {
                exec_pipeline.add(Box::new(SampleNode));
            }
            ast::QueryMode::RelevanceFeedback => {
                let feedback: Vec<(Value<'static>, f64)> = stmt
                    .feedback_items
                    .iter()
                    .map(|item| {
                        let example = clone_value(&item.example);
                        (example, item.score)
                    })
                    .collect();
                let strategy = stmt.feedback_strategy.as_ref().map(|s| (s.a, s.b, s.c));
                let target = clone_value(stmt.feedback_target.as_ref().unwrap_or(&Value::Null));
                exec_pipeline.add(Box::new(RelevanceFeedbackNode {
                    target,
                    feedback,
                    strategy,
                }));
            }
            ast::QueryMode::Nearest => {
                if !stmt.raw_vector.is_empty() {
                    if stmt.query_type == ast::QueryType::Hybrid {
                        if stmt.query_text.is_none() {
                            return Err(QqlError::runtime(
                                "USING HYBRID with a raw dense vector requires a text query for the sparse vector",
                            ));
                        }
                        exec_pipeline.add(Box::new(RawVectorNode {
                            vector: stmt.raw_vector.clone(),
                            vector_name: dense_vector_name.clone(),
                            limit: stmt.limit as u64 * 10,
                            as_prefetch: true,
                        }));
                        exec_pipeline.add(Box::new(SparseEmbedNode {
                            model: sparse_model.clone().unwrap_or_else(|| {
                                crate::executor::SPARSE_MODEL_DEFAULT.to_string()
                            }),
                            vector_name: sparse_vector_name.clone(),
                            limit: stmt.limit as u64 * 10,
                            as_prefetch: true,
                        }));
                        exec_pipeline.add(Box::new(FusionNode {
                            mode: stmt.fusion_type.unwrap_or("rrf").to_string(),
                        }));
                    } else {
                        exec_pipeline.add(Box::new(RawVectorNode {
                            vector: stmt.raw_vector.clone(),
                            vector_name: dense_vector_name.clone(),
                            limit: stmt.limit as u64,
                            as_prefetch: false,
                        }));
                        if let Some(fusion_type) = &stmt.fusion_type {
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: fusion_type.to_string(),
                            }));
                            state.vector_name = String::new();
                        }
                    }
                } else if let Some(query_id) = &stmt.query_id {
                    let id = clone_value(query_id);
                    exec_pipeline.add(Box::new(RecommendNode {
                        positive_ids: vec![id],
                        negative_ids: Vec::new(),
                        strategy: stmt.strategy.map(|s| s.to_string()),
                    }));
                } else {
                    if let Some(text) = &stmt.query_text {
                        state.query_text = text.to_string();
                    }
                    match stmt.query_type {
                        ast::QueryType::Hybrid => {
                            exec_pipeline.add(Box::new(DenseEmbedNode {
                                model: dense_model.clone(),
                                vector_name: dense_vector_name.clone(),
                                limit: stmt.limit as u64 * 10,
                                as_prefetch: true,
                            }));
                            exec_pipeline.add(Box::new(SparseEmbedNode {
                                model: sparse_model.clone().unwrap_or_else(|| {
                                    crate::executor::SPARSE_MODEL_DEFAULT.to_string()
                                }),
                                vector_name: sparse_vector_name.clone(),
                                limit: stmt.limit as u64 * 10,
                                as_prefetch: true,
                            }));
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: stmt.fusion_type.unwrap_or("rrf").to_string(),
                            }));
                        }
                        ast::QueryType::Sparse => {
                            let sm = self.resolve_sparse_model(stmt.model);
                            exec_pipeline.add(Box::new(SparseEmbedNode {
                                model: sm.clone(),
                                vector_name: sparse_vector_name.clone(),
                                limit: stmt.limit as u64,
                                as_prefetch: false,
                            }));
                            state.vector_name = sparse_vector_name.clone();
                        }
                        ast::QueryType::Dense => {
                            if stmt.query_text.is_some() {
                                exec_pipeline.add(Box::new(DenseEmbedNode {
                                    model: dense_model.clone(),
                                    vector_name: dense_vector_name.clone(),
                                    limit: stmt.limit as u64,
                                    as_prefetch: false,
                                }));
                                state.vector_name = dense_vector_name.clone();
                            }
                        }
                    }
                    if let Some(fusion_type) = &stmt.fusion_type {
                        if stmt.query_type != ast::QueryType::Hybrid {
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: fusion_type.to_string(),
                            }));
                            state.vector_name = String::new();
                        }
                    }
                }
            }
            ast::QueryMode::Recommend => {
                let pos: Vec<Value<'static>> = stmt.positive_ids.iter().map(clone_value).collect();
                let neg: Vec<Value<'static>> = stmt.negative_ids.iter().map(clone_value).collect();
                exec_pipeline.add(Box::new(RecommendNode {
                    positive_ids: pos,
                    negative_ids: neg,
                    strategy: stmt.strategy.map(|s| s.to_string()),
                }));
                state.vector_name = dense_vector_name.clone();
            }
            ast::QueryMode::Context => {
                let pairs: Vec<pipeline::ContextPairInput> = stmt
                    .context_pairs
                    .iter()
                    .map(|p| pipeline::ContextPairInput {
                        positive: Some(clone_value(&p.positive)),
                        negative: Some(clone_value(&p.negative)),
                    })
                    .collect();
                exec_pipeline.add(Box::new(pipeline::ContextNode { pairs }));
                state.vector_name = dense_vector_name.clone();
            }
            ast::QueryMode::Discover => {
                let pairs: Vec<pipeline::ContextPairInput> = stmt
                    .context_pairs
                    .iter()
                    .map(|p| pipeline::ContextPairInput {
                        positive: Some(clone_value(&p.positive)),
                        negative: Some(clone_value(&p.negative)),
                    })
                    .collect();
                exec_pipeline.add(Box::new(DiscoverNode {
                    target: stmt.target.as_ref().map(clone_value),
                    pairs,
                }));
                state.vector_name = dense_vector_name.clone();
            }
        }

        if let Some(ref formula) = stmt.formula {
            let formula_json = crate::pipeline::build_expression(formula)?;
            let mut defaults = Vec::new();
            for (k, v) in &stmt.formula_defaults {
                let val_f64 = match v {
                    Value::Int(i) => *i as f64,
                    Value::Float(f) => *f,
                    _ => 0.0,
                };
                defaults.push((k.to_string(), val_f64));
            }
            exec_pipeline.add(Box::new(crate::pipeline::formula_nodes::FormulaNode {
                expr: formula_json,
                defaults,
            }));
        }

        if stmt.rerank {
            let rerank_model = stmt.rerank_model.unwrap_or("default-reranker");
            exec_pipeline.add(Box::new(RerankNode {
                model: rerank_model.to_string(),
            }));
        }

        Ok((state, exec_pipeline))
    }

    pub(crate) async fn do_query(
        &self,
        stmt: ast::QueryStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let (mut state, exec_pipeline) = self.build_query_state_and_pipeline(&stmt).await?;

        exec_pipeline.execute(&mut state).await?;

        if !state.group_by.is_empty() {
            self.execute_grouped_query(&exec_pipeline, &state).await
        } else {
            self.execute_flat_query(&exec_pipeline, &state).await
        }
    }

    pub(crate) async fn execute_flat_query(
        &self,
        pipeline: &QueryPipeline,
        state: &QueryState,
    ) -> Result<ExecResponse, QqlError> {
        let mut req = pipeline.build_flat_request(state)?;
        if req.with_payload.is_none() {
            req.with_payload = Some(
                WithPayload {
                    enable: Some(true),
                    include: Vec::new(),
                    exclude: Vec::new(),
                }
                .into(),
            );
        }
        let results = self.client.query(req).await?;

        let formatted: Vec<SearchHit> = results
            .into_iter()
            .map(|hit| {
                let payload_map: Option<HashMap<String, serde_json::Value>> = hit
                    .payload
                    .as_ref()
                    .and_then(|p| serde_json::from_value(serde_json::to_value(p).unwrap()).ok());
                SearchHit {
                    id: point_id_string(&hit.id.clone().into()),
                    score: hit.score,
                    text: payload_map.as_ref().and_then(|p| {
                        p.get("text")
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                    }),
                    payload: payload_map,
                }
            })
            .collect();

        Ok(ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: format!("Found {} hits", formatted.len()),
            data: Some(serde_json::to_value(formatted).unwrap_or(serde_json::Value::Null)),
        })
    }

    pub(crate) async fn execute_grouped_query(
        &self,
        pipeline: &QueryPipeline,
        state: &QueryState,
    ) -> Result<ExecResponse, QqlError> {
        let mut req = pipeline.build_grouped_request(state)?;
        if req.with_payload.is_none() {
            req.with_payload = Some(
                WithPayload {
                    enable: Some(true),
                    include: Vec::new(),
                    exclude: Vec::new(),
                }
                .into(),
            );
        }
        let groups = self.client.query_groups(req).await?;

        let formatted: Vec<crate::executor::GroupedSearchResult> = groups
            .into_iter()
            .map(|g| {
                let hits: Vec<SearchHit> = g
                    .hits
                    .into_iter()
                    .map(|hit| {
                        let payload_map: Option<HashMap<String, serde_json::Value>> =
                            hit.payload.as_ref().and_then(|p| {
                                serde_json::from_value(serde_json::to_value(p).unwrap()).ok()
                            });
                        SearchHit {
                            id: point_id_string(&hit.id.clone().into()),
                            score: hit.score,
                            text: payload_map.as_ref().and_then(|p| {
                                p.get("text")
                                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                            }),
                            payload: payload_map,
                        }
                    })
                    .collect();
                crate::executor::GroupedSearchResult {
                    group_id: serde_json::to_value(&g.id).unwrap_or(serde_json::Value::Null),
                    hits,
                }
            })
            .collect();

        Ok(ExecResponse {
            ok: true,
            operation: "QUERY_GROUPS".to_string(),
            message: format!("Found {} groups", formatted.len()),
            data: Some(serde_json::to_value(formatted).unwrap_or(serde_json::Value::Null)),
        })
    }
}
