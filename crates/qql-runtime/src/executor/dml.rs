use serde_json;
use std::collections::HashMap;

use crate::embedder::HttpEmbedder;
use crate::executor::{
    CreateCollectionReq, DeletePointsReq, ExecResponse, Executor, GetPointsReq, PointId,
    PointStruct, ScrollPointsReq, SearchHit, SetPayloadReq, UpdateVectorsReq, UpsertPointsReq,
    VectorTopology,
};
use crate::filter_conv::FilterConverter;
use crate::pipeline::{
    self, DenseEmbedNode, DiscoverNode, FusionNode, OrderByNode, QueryPipeline, QueryState,
    RawVectorNode, RecommendNode, RelevanceFeedbackNode, RerankNode, SampleNode, SparseEmbedNode,
    WithPayload,
};
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

use super::helpers::{
    build_with_payload, build_with_vectors, clone_value, has_mmr, point_id_string,
    to_point_id_static, value_to_json,
};

impl Executor {
    async fn build_query_state_and_pipeline(
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
                            model: sparse_model
                                .clone()
                                .unwrap_or_else(|| super::SPARSE_MODEL_DEFAULT.to_string()),
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
                                model: sparse_model
                                    .clone()
                                    .unwrap_or_else(|| super::SPARSE_MODEL_DEFAULT.to_string()),
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
        let mut req = pipeline.build_flat_request(state);
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
            .map(|hit| SearchHit {
                id: point_id_string(&hit.id),
                score: hit.score,
                text: hit.payload.as_ref().and_then(|p| {
                    p.get("text")
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                }),
                payload: hit.payload,
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
        let mut req = pipeline.build_grouped_request(state);
        if req.with_payload.is_none() {
            req.with_payload = Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }
        let groups = self.client.query_groups(req).await?;

        let formatted: Vec<super::GroupedSearchResult> = groups
            .into_iter()
            .map(|g| {
                let hits: Vec<SearchHit> = g
                    .hits
                    .into_iter()
                    .map(|hit| SearchHit {
                        id: point_id_string(&hit.id),
                        score: hit.score,
                        text: hit.payload.as_ref().and_then(|p| {
                            p.get("text")
                                .and_then(|v| v.as_str().map(|s| s.to_string()))
                        }),
                        payload: hit.payload,
                    })
                    .collect();
                super::GroupedSearchResult {
                    group_id: g.group_id,
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

    pub(crate) fn resolve_dense_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if !cfg.embedding_model.as_deref().unwrap_or("").is_empty() {
                return cfg.embedding_model.as_ref().unwrap().clone();
            }
            if !cfg.inference_model.as_deref().unwrap_or("").is_empty() {
                return cfg.inference_model.as_ref().unwrap().clone();
            }
        }
        super::DENSE_MODEL_DEFAULT.to_string()
    }

    pub(crate) fn resolve_sparse_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if let Some(ref sm) = cfg.sparse_inference_model {
                if !sm.is_empty() {
                    return sm.clone();
                }
            }
        }
        super::SPARSE_MODEL_DEFAULT.to_string()
    }

    pub(crate) fn inference_mode(&self) -> String {
        if let Some(ref cfg) = self.config {
            let mode = cfg.inference_mode.trim();
            if !mode.is_empty() {
                return mode.to_lowercase();
            }
        }
        super::INFERENCE_MODE_DEFAULT.to_string()
    }

    pub(crate) fn uses_local_embeddings(&self) -> bool {
        let mode = self.inference_mode();
        mode == "local" || mode == "external"
    }

    pub(crate) fn cloud_model_options(&self) -> HashMap<String, String> {
        if self.uses_local_embeddings() {
            return HashMap::new();
        }
        self.config
            .as_ref()
            .map(|c| c.cloud_model_options.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn resolve_vector_topology(
        &self,
        collection: &str,
    ) -> Result<VectorTopology, QqlError> {
        let info = self.client.get_collection_info(collection).await?;
        let mut topo = VectorTopology {
            dense_vector: None,
            sparse_vector: None,
            rerank_vector: None,
        };

        if let Some(ref config) = info.config {
            if let Some(ref params) = config.params {
                if let Some(ref vc) = params.vectors_config {
                    match vc {
                        super::VectorsConfigType::Multi(map) => {
                            for vname in map.keys() {
                                if vname == super::DENSE_VECTOR_NAME {
                                    topo.dense_vector = Some(super::DENSE_VECTOR_NAME.to_string());
                                } else if vname == super::RERANK_VECTOR_NAME {
                                    topo.rerank_vector =
                                        Some(super::RERANK_VECTOR_NAME.to_string());
                                } else if topo.dense_vector.is_none()
                                    || topo
                                        .dense_vector
                                        .as_ref()
                                        .map(|s| s.is_empty())
                                        .unwrap_or(true)
                                {
                                    topo.dense_vector = Some(vname.clone());
                                }
                            }
                        }
                        super::VectorsConfigType::Single(_) => {
                            topo.dense_vector = Some(String::new());
                        }
                    }
                }

                if let Some(ref svc) = params.sparse_vectors_config {
                    for vname in svc.keys() {
                        if vname == super::SPARSE_VECTOR_NAME {
                            topo.sparse_vector = Some(super::SPARSE_VECTOR_NAME.to_string());
                        } else if topo.sparse_vector.is_none()
                            || topo
                                .sparse_vector
                                .as_ref()
                                .map(|s| s.is_empty())
                                .unwrap_or(true)
                        {
                            topo.sparse_vector = Some(vname.clone());
                        }
                    }
                }
            }
        }

        Ok(topo)
    }

    pub(crate) async fn ensure_collection_for_insert(
        &self,
        collection: &str,
        model: Option<&str>,
        requested_hybrid: bool,
        explicit_dense: Option<&str>,
        explicit_sparse: Option<&str>,
    ) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if exists {
            return Ok(false);
        }

        let dense_size = self.resolve_dense_vector_size(model).await?;
        let dense_name = explicit_dense.unwrap_or(super::DENSE_VECTOR_NAME);

        let mut create_req = CreateCollectionReq::new(collection.to_string());
        create_req.vectors_config = Some(serde_json::json!({
            dense_name: {
                "size": dense_size,
                "distance": "Cosine"
            }
        }));

        if requested_hybrid {
            let sparse_name = explicit_sparse.unwrap_or(super::SPARSE_VECTOR_NAME);
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {
                    "modifier": "idf"
                }
            }));
        }

        self.client.create_collection(create_req).await?;
        // Wait for collection to be ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(true)
    }

    pub(crate) async fn resolve_dense_vector_size(
        &self,
        model: Option<&str>,
    ) -> Result<usize, QqlError> {
        if self.uses_local_embeddings() {
            if let Some(ref cfg) = self.config {
                if cfg.embedding_dimension > 0 {
                    return Ok(cfg.embedding_dimension);
                }
            }
            return match self.config.as_ref() {
                Some(cfg)
                    if !cfg.embedding_endpoint.as_deref().unwrap_or("").is_empty()
                        && !cfg.embedding_model.as_deref().unwrap_or("").is_empty() =>
                {
                    let embedder = HttpEmbedder::new(
                        cfg.embedding_endpoint.clone().unwrap_or_default(),
                        cfg.embedding_api_key.clone().unwrap_or_default(),
                        cfg.embedding_model.clone().unwrap_or_default(),
                        1,
                    )?;
                    let dim = embedder.probe_dimension("probe").await?;
                    Ok(dim)
                }
                _ => Err(QqlError::runtime(
                    "embedding_dimension must be configured for local inference mode",
                )),
            };
        }

        if let Some(ref cfg) = self.config {
            if cfg.embedding_dimension > 0 {
                return Ok(cfg.embedding_dimension);
            }
        }

        if model.is_some()
            && model.unwrap() != ""
            && self
                .config
                .as_ref()
                .map(|c| c.embedding_dimension == 0)
                .unwrap_or(true)
        {
            return Err(QqlError::runtime(
                "embedding_dimension must be configured when creating collections with USING MODEL",
            ));
        }

        Ok(super::DENSE_VECTOR_SIZE as usize)
    }

    pub(crate) async fn do_insert(
        &self,
        stmt: ast::InsertStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let _created = self
            .ensure_collection_for_insert(
                stmt.collection,
                stmt.model,
                stmt.hybrid,
                stmt.dense_vector,
                stmt.sparse_vector,
            )
            .await?;

        let mut points = Vec::with_capacity(stmt.values_list.len());

        for row in &stmt.values_list {
            let payload: HashMap<String, serde_json::Value> = row
                .iter()
                .map(|(k, v)| (k.to_string(), value_to_json(v)))
                .collect();

            let mut point = PointStruct {
                id: PointId::Num(0),
                vector: None,
                payload: Some(payload),
            };

            // Extract ID from payload if present
            let id_val = row.iter().find(|(k, _)| *k == "id");
            if let Some((_, Value::Int(id))) = id_val {
                point.id = PointId::Num(*id as u64);
            } else if let Some((_, Value::Str(id))) = id_val {
                point.id = PointId::Uuid(id.to_string());
            }

            // Determine text field for embedding
            let text_field = row
                .iter()
                .find(|(k, _)| *k == "text" || *k == "description" || *k == "content")
                .map(|(_, v)| match v {
                    Value::Str(s) => s.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();

            if !text_field.is_empty() && self.uses_local_embeddings() {
                let _dense_dim = self.resolve_dense_vector_size(stmt.model).await?;
                if let Some(ref embedder) = self.embedder {
                    let dense_vec = embedder
                        .embed_dense(&text_field, &self.resolve_dense_model(stmt.model))
                        .await?;
                    point.vector = Some(serde_json::json!({
                        stmt.dense_vector.unwrap_or(super::DENSE_VECTOR_NAME): dense_vec
                    }));
                }
            }

            points.push(point);
        }

        let req = UpsertPointsReq {
            collection_name: stmt.collection.to_string(),
            points,
        };

        self.client.upsert(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "insert".to_string(),
            message: format!("Inserted {} point(s)", stmt.values_list.len()),
            data: Some(serde_json::json!({"count": stmt.values_list.len()})),
        })
    }

    pub(crate) async fn do_select(
        &self,
        stmt: ast::SelectStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let req = GetPointsReq {
            collection_name: stmt.collection.to_string(),
            point_id: clone_value(&stmt.point_id),
        };
        let results = self.client.get(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "select".to_string(),
            message: format!("Found {} point(s)", results.len()),
            data: Some(serde_json::to_value(&results).unwrap_or(serde_json::Value::Null)),
        })
    }

    pub(crate) async fn do_scroll(
        &self,
        stmt: ast::ScrollStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(filter)?
        } else {
            None
        };

        let after = stmt
            .after
            .as_ref()
            .map(|v| to_point_id_static(v))
            .transpose()?;

        let req = ScrollPointsReq {
            collection_name: stmt.collection.to_string(),
            limit: stmt.limit as u64,
            filter: qdrant_filter,
            after,
        };

        let (points, next_offset) = self.client.scroll(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "scroll".to_string(),
            message: format!("Found {} point(s)", points.len()),
            data: Some(serde_json::json!({
                "points": points,
                "next_offset": next_offset.map(|p| point_id_string(&p)),
            })),
        })
    }

    pub(crate) async fn do_delete(
        &self,
        stmt: ast::DeleteStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let mut filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        if let Some(ref field) = stmt.field {
            if let Some(ref val) = stmt.value {
                let f_val = match val {
                    Value::Str(s) => crate::filter_conv::FilterValue::Str(s.to_string()),
                    Value::Int(i) => crate::filter_conv::FilterValue::Int(*i),
                    Value::Float(f) => crate::filter_conv::FilterValue::Float(*f),
                    Value::Bool(b) => crate::filter_conv::FilterValue::Bool(*b),
                    _ => {
                        return Err(QqlError::runtime(
                            "unsupported value type for delete filter",
                        ))
                    }
                };
                let cond = crate::filter_conv::QdrantCondition::Match {
                    key: field.to_string(),
                    value: f_val,
                };
                if let Some(ref mut f) = filter {
                    if let Some(ref mut musts) = f.must {
                        musts.push(cond);
                    } else {
                        f.must = Some(vec![cond]);
                    }
                } else {
                    filter = Some(crate::filter_conv::QdrantFilter {
                        must: Some(vec![cond]),
                        must_not: None,
                        should: None,
                    });
                }
            }
        }

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let req = DeletePointsReq {
            collection_name: stmt.collection.to_string(),
            filter,
            point_id,
        };

        self.client.delete(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "delete".to_string(),
            message: "Points deleted".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_update_vector(
        &self,
        stmt: ast::UpdateVectorStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let point_id = to_point_id_static(&stmt.point_id)?;

        let req = UpdateVectorsReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            vector: stmt.vector.clone(),
            vector_name: stmt.vector_name.map(|s| s.to_string()),
        };

        self.client.update_vectors(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_vector".to_string(),
            message: "Vector updated".to_string(),
            data: None,
        })
    }

    pub(crate) async fn do_update_payload(
        &self,
        stmt: ast::UpdatePayloadStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let payload: HashMap<String, serde_json::Value> = stmt
            .payload
            .iter()
            .map(|(k, v)| (k.to_string(), value_to_json(v)))
            .collect();

        let req = SetPayloadReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            filter,
            payload,
        };

        self.client.set_payload(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_payload".to_string(),
            message: "Payload updated".to_string(),
            data: None,
        })
    }
}
