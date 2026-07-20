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
    build_with_payload, build_with_vector, clone_value, has_mmr, point_id_string,
};

impl Executor {
    pub(crate) async fn build_query_state_and_pipeline(
        &self,
        stmt: &ast::QueryStmt,
    ) -> Result<(QueryState, QueryPipeline), QqlError> {
        let dense_vector_name: String;
        let sparse_vector_name: String;

        if let Some(using) = &stmt.using_ {
            dense_vector_name = using.clone();
            sparse_vector_name = using.clone();
        } else {
            let topo = self
                .resolve_vector_topology(stmt.collection.as_deref().unwrap_or(""))
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

        let dense_model = self.resolve_dense_model(stmt.model.as_deref());
        let sparse_model = if stmt.query_type == ast::QueryType::Hybrid {
            Some(self.resolve_sparse_model(stmt.model.as_deref()))
        } else {
            None
        };

        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter
                .build_filter(filter)?
                .map(crate::backend::Filter::from_json)
        } else {
            None
        };

        let mut state = QueryState {
            query_text: stmt.query_text.clone().unwrap_or_default(),
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

            collection_name: stmt.collection.clone().unwrap_or_default(),
            vector_name: String::new(),
            limit: stmt.limit as u64,
            offset: stmt.offset as u64,
            qdrant_filter,
            score_threshold: stmt.score_threshold.map(|v| v as f32),
            lookup_from: stmt
                .lookup_from
                .as_ref()
                .map(|lf| pipeline::LookupLocation {
                    collection_name: lf.clone(),
                    vector_name: stmt.lookup_vector.clone(),
                }),
            with_payload: build_with_payload(stmt.with_payload.as_ref().map(|p| p.as_ref())),
            with_vector: build_with_vector(stmt.with_vector.as_ref().map(|v| v.as_ref())),
            group_by: stmt.group_by.clone().unwrap_or_default(),
            group_size: stmt.group_size.unwrap_or(0) as u64,
            with_lookup: stmt
                .with_lookup_collection
                .as_ref()
                .map(|c| pipeline::WithLookup {
                    collection: c.clone(),
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

        // Resolve CTEs
        let mut cte_map = HashMap::new();
        for cte in &stmt.ctes {
            let pq = Box::pin(self.build_cte_prefetch(cte.stmt.as_ref(), &cte_map)).await?;
            cte_map.insert(cte.name.to_string(), pq);
        }

        // Populate manual_prefetches from prefetch_refs
        for ref_node in &stmt.prefetch_refs {
            let pq = cte_map.get(ref_node.cte_name.as_str()).ok_or_else(|| {
                QqlError::runtime(format!(
                    "unknown CTE referenced in prefetch: '{}'",
                    ref_node.cte_name
                ))
            })?;

            let mut clone = pq.clone();
            if let Some(ref filter) = ref_node.filter {
                let converter = FilterConverter::new();
                clone.filter = converter
                    .build_filter(filter)?
                    .map(crate::backend::Filter::from_json);
            }
            if let Some(st) = ref_node.score_threshold {
                clone.score_threshold = Some(st as f32);
            }
            if let Some(lf) = &ref_node.lookup_from {
                clone.lookup_from = Some(pipeline::LookupLocation {
                    collection_name: lf.clone(),
                    vector_name: ref_node.lookup_vector.clone(),
                });
            }
            state.manual_prefetches.push(clone);
        }

        let mut exec_pipeline = QueryPipeline::new();

        match stmt.mode {
            ast::QueryMode::OrderBy => {
                let asc = stmt.order_by_asc.unwrap_or(true);
                exec_pipeline.add(Box::new(OrderByNode {
                    field: stmt.order_by_field.clone().unwrap_or_default(),
                    asc,
                }));
            }
            ast::QueryMode::Sample => {
                exec_pipeline.add(Box::new(SampleNode));
            }
            ast::QueryMode::RelevanceFeedback => {
                let feedback: Vec<(Value, f64)> = stmt
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
                            mode: stmt.fusion_type.as_deref().unwrap_or("rrf").to_string(),
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
                        strategy: stmt.strategy.clone(),
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
                                mode: stmt.fusion_type.as_deref().unwrap_or("rrf").to_string(),
                            }));
                        }
                        ast::QueryType::Sparse => {
                            let sm = self.resolve_sparse_model(stmt.model.as_deref());
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
                let pos: Vec<Value> = stmt.positive_ids.iter().map(clone_value).collect();
                let neg: Vec<Value> = stmt.negative_ids.iter().map(clone_value).collect();
                exec_pipeline.add(Box::new(RecommendNode {
                    positive_ids: pos,
                    negative_ids: neg,
                    strategy: stmt.strategy.clone(),
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
            let rerank_model = stmt.rerank_model.as_deref().unwrap_or("default-reranker");
            exec_pipeline.add(Box::new(RerankNode {
                model: rerank_model.to_string(),
            }));
        }

        Ok((state, exec_pipeline))
    }

    pub(crate) async fn do_query(&self, stmt: ast::QueryStmt) -> Result<ExecResponse, QqlError> {
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
            req.with_payload = Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }
        let results = self.client.query(req).await?;

        let formatted: Vec<SearchHit> = results
            .into_iter()
            .map(|hit| {
                let payload_map = hit.payload.clone();
                SearchHit {
                    id: point_id_string(&hit.id),
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
            req.with_payload = Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }
        let groups = self.client.query_groups(req).await?;

        let formatted: Vec<crate::executor::GroupedSearchResult> = groups
            .into_iter()
            .map(|g| {
                let hits: Vec<SearchHit> = g
                    .hits
                    .into_iter()
                    .map(|hit| {
                        let payload_map = hit.payload.clone();
                        SearchHit {
                            id: point_id_string(&hit.id),
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

    async fn build_cte_prefetch(
        &self,
        stmt: &ast::QueryStmt,
        cte_map: &HashMap<String, pipeline::PrefetchQuery>,
    ) -> Result<pipeline::PrefetchQuery, QqlError> {
        let mut prefetches = Vec::new();

        let mut scoped_map = cte_map.clone();
        for local_cte in &stmt.ctes {
            let local_pq =
                Box::pin(self.build_cte_prefetch(local_cte.stmt.as_ref(), &scoped_map)).await?;
            scoped_map.insert(local_cte.name.to_string(), local_pq);
        }

        for ref_node in &stmt.prefetch_refs {
            let nested = scoped_map.get(ref_node.cte_name.as_str()).ok_or_else(|| {
                QqlError::runtime(format!(
                    "unknown CTE referenced in prefetch: '{}'",
                    ref_node.cte_name
                ))
            })?;
            prefetches.push(nested.clone());
        }

        let using = stmt.using_.clone();
        let limit = if stmt.limit > 0 {
            Some(stmt.limit as u64)
        } else {
            None
        };
        let score_threshold = stmt.score_threshold.map(|v| v as f32);
        let lookup_from = stmt
            .lookup_from
            .as_ref()
            .map(|lf| pipeline::LookupLocation {
                collection_name: lf.clone(),
                vector_name: stmt.lookup_vector.clone(),
            });

        let filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter
                .build_filter(f)?
                .map(crate::backend::Filter::from_json)
        } else {
            None
        };

        let params = stmt
            .with_clause
            .as_ref()
            .and_then(|wc| pipeline::build_search_params(wc));

        let dense_model = self.resolve_dense_model(stmt.model.as_deref());

        let mut query = None;
        match stmt.mode {
            ast::QueryMode::Recommend => {
                let mut pos = Vec::new();
                for id in &stmt.positive_ids {
                    let pid = crate::pipeline::to_point_id(id)?;
                    pos.push(pipeline::VectorInput::Id(pid));
                }
                let mut neg = Vec::new();
                for id in &stmt.negative_ids {
                    let pid = crate::pipeline::to_point_id(id)?;
                    neg.push(pipeline::VectorInput::Id(pid));
                }
                let strategy = if let Some(strat) = &stmt.strategy {
                    match strat.to_lowercase().as_str() {
                        "average_vector" => Some(pipeline::RecommendStrategyType::AverageVector),
                        "best_score" => Some(pipeline::RecommendStrategyType::BestScore),
                        "sum_scores" => Some(pipeline::RecommendStrategyType::SumScores),
                        _ => {
                            return Err(QqlError::runtime(format!(
                                "unknown recommend strategy '{}'",
                                strat
                            )))
                        }
                    }
                } else {
                    None
                };
                query = Some(pipeline::QueryVariant::Recommend(
                    pipeline::RecommendInput {
                        positive: pos,
                        negative: neg,
                        strategy,
                    },
                ));
            }
            ast::QueryMode::Nearest => {
                if stmt.query_type == ast::QueryType::Hybrid {
                    return Err(QqlError::runtime(
                        "USING HYBRID is not supported inside CTE prefetch queries; define separate sparse and dense CTEs and combine them via prefetch references",
                    ));
                }

                if !stmt.raw_vector.is_empty() {
                    query = Some(pipeline::QueryVariant::Nearest(
                        stmt.raw_vector.iter().map(|&x| x as f32).collect(),
                    ));
                } else if let Some(text) = &stmt.query_text {
                    let is_sparse = stmt.query_type == ast::QueryType::Sparse;
                    if is_sparse {
                        if self.uses_local_embeddings() {
                            let embedder = self.embedder.as_ref().ok_or_else(|| {
                                QqlError::runtime(
                                    "local embedding requested but no Embedder provided",
                                )
                            })?;
                            let sv = embedder.embed_sparse(text).await?;
                            query = Some(pipeline::QueryVariant::Sparse(sv.indices, sv.values));
                        } else {
                            query = Some(pipeline::QueryVariant::Document {
                                text: text.to_string(),
                                model: self.resolve_sparse_model(stmt.model.as_deref()),
                                options: self.cloud_model_options(),
                            });
                        }
                    } else {
                        if self.uses_local_embeddings() {
                            let embedder = self.embedder.as_ref().ok_or_else(|| {
                                QqlError::runtime(
                                    "local embedding requested but no Embedder provided",
                                )
                            })?;
                            let dv = embedder.embed_dense(text, &dense_model).await?;
                            query = Some(pipeline::QueryVariant::Nearest(dv));
                        } else {
                            query = Some(pipeline::QueryVariant::Document {
                                text: text.to_string(),
                                model: dense_model.clone(),
                                options: self.cloud_model_options(),
                            });
                        }
                    }
                } else if let Some(ref query_id) = stmt.query_id {
                    let pid = crate::pipeline::to_point_id(query_id)?;
                    query = Some(pipeline::QueryVariant::Recommend(
                        pipeline::RecommendInput {
                            positive: vec![pipeline::VectorInput::Id(pid)],
                            negative: Vec::new(),
                            strategy: None,
                        },
                    ));
                }
            }
            _ => {}
        }

        Ok(pipeline::PrefetchQuery {
            prefetches,
            query,
            using,
            limit,
            params,
            filter,
            score_threshold,
            lookup_from,
        })
    }
}
