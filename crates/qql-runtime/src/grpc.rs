use async_trait::async_trait;
use tonic::transport::Channel;

use crate::backend::{
    CollectionParamsSpec, CollectionSchema, PayloadIndexSpec, SparseVectorSpec, VectorSpec,
};
use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

pub struct GrpcQdrant {
    channel: Channel,
    api_key: Option<String>,
}

/// Interceptor that attaches the Qdrant API key metadata header (RUN-009).
#[derive(Clone)]
struct ApiKeyInterceptor {
    api_key: Option<String>,
}

impl tonic::service::Interceptor for ApiKeyInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref key) = self.api_key {
            let value = tonic::metadata::MetadataValue::try_from(key.as_str())
                .map_err(|e| tonic::Status::invalid_argument(format!("invalid api key: {e}")))?;
            request.metadata_mut().insert("api-key", value);
        }
        Ok(request)
    }
}

impl GrpcQdrant {
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            channel,
            api_key: None,
        }
    }

    pub fn from_url(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Self::from_url_with_timeout(url, api_key, None)
    }

    pub fn from_url_with_timeout(
        url: &str,
        api_key: Option<String>,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self, QqlError> {
        let scheme = if url.starts_with("https://") {
            "https://"
        } else {
            "http://"
        };
        let raw = url
            .trim_start_matches("grpc://")
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let endpoint = format!("{scheme}{raw}");

        let mut ep = tonic::transport::Endpoint::from_shared(endpoint).map_err(|e| {
            QqlError::transport("QQL-TRANSPORT", format!("invalid gRPC url: {e}"), None)
        })?;
        if let Some(t) = timeout {
            ep = ep.timeout(t);
        }
        let channel = ep.connect_lazy();

        Ok(Self { channel, api_key })
    }

    pub fn channel(&self) -> Channel {
        self.channel.clone()
    }

    fn points_client(
        &self,
    ) -> qdrant::points_client::PointsClient<
        tonic::service::interceptor::InterceptedService<Channel, ApiKeyInterceptor>,
    > {
        qdrant::points_client::PointsClient::with_interceptor(
            self.channel.clone(),
            ApiKeyInterceptor {
                api_key: self.api_key.clone(),
            },
        )
    }

    fn collections_client(
        &self,
    ) -> qdrant::collections_client::CollectionsClient<
        tonic::service::interceptor::InterceptedService<Channel, ApiKeyInterceptor>,
    > {
        qdrant::collections_client::CollectionsClient::with_interceptor(
            self.channel.clone(),
            ApiKeyInterceptor {
                api_key: self.api_key.clone(),
            },
        )
    }

    // ── Thin typed wrappers — same API shape as qdrant-client's Qdrant ──

    pub async fn query(&self, req: qdrant::QueryPoints) -> Result<qdrant::QueryResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))
    }

    pub async fn query_groups(
        &self,
        req: qdrant::QueryPointGroups,
    ) -> Result<qdrant::QueryGroupsResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_groups(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))
    }

    pub async fn query_batch(
        &self,
        req: qdrant::QueryBatchPoints,
    ) -> Result<qdrant::QueryBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))
    }

    pub async fn update_batch(
        &self,
        req: qdrant::UpdateBatchPoints,
    ) -> Result<qdrant::UpdateBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))
    }

    pub async fn get_points(
        &self,
        req: qdrant::GetPoints,
    ) -> Result<qdrant::GetResponse, QqlError> {
        let mut cl = self.points_client();
        cl.get(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))
    }

    pub async fn scroll(
        &self,
        req: qdrant::ScrollPoints,
    ) -> Result<qdrant::ScrollResponse, QqlError> {
        let mut cl = self.points_client();
        cl.scroll(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))
    }

    pub async fn upsert_points(
        &self,
        req: qdrant::UpsertPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.upsert(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))
    }

    pub async fn delete_points(
        &self,
        req: qdrant::DeletePoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))
    }

    pub async fn update_vectors(
        &self,
        req: qdrant::UpdatePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))
    }

    pub async fn set_payload(
        &self,
        req: qdrant::SetPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.set_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))
    }

    pub async fn create_collection_raw(
        &self,
        req: qdrant::CreateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))
    }

    pub async fn update_collection_raw(
        &self,
        req: qdrant::UpdateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.update(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None))
    }

    pub async fn delete_collection_raw(
        &self,
        req: qdrant::DeleteCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None))
    }

    pub async fn create_field_index(
        &self,
        req: qdrant::CreateFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.create_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_field_index: {e}"), None))
    }

    pub async fn delete_field_index(
        &self,
        req: qdrant::DeleteFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_field_index: {e}"), None))
    }

    pub async fn count_points(
        &self,
        req: qdrant::CountPoints,
    ) -> Result<qdrant::CountResponse, QqlError> {
        let mut cl = self.points_client();
        cl.count(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))
    }

    pub async fn clear_payload(
        &self,
        req: qdrant::ClearPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.clear_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))
    }

    pub async fn delete_vectors(
        &self,
        req: qdrant::DeletePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))
    }

    pub async fn create_shard_key(
        &self,
        req: qdrant::CreateShardKeyRequest,
    ) -> Result<qdrant::CreateShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None))
    }

    pub async fn delete_shard_key(
        &self,
        req: qdrant::DeleteShardKeyRequest,
    ) -> Result<qdrant::DeleteShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_shard_key: {e}"), None))
    }

    pub async fn list_shard_keys(
        &self,
        req: qdrant::ListShardKeysRequest,
    ) -> Result<qdrant::ListShardKeysResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list_shard_keys(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None))
    }

    pub async fn list_collections_raw(&self) -> Result<qdrant::ListCollectionsResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list(tonic::Request::new(qdrant::ListCollectionsRequest {}))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_collections: {e}"), None))
    }

    pub async fn collection_info_raw(
        &self,
        collection: String,
    ) -> Result<qdrant::GetCollectionInfoResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.get(tonic::Request::new(qdrant::GetCollectionInfoRequest {
            collection_name: collection,
        }))
        .await
        .map(|r| r.into_inner())
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("collection_info: {e}"), None))
    }
}

#[async_trait]
impl QdrantOps for GrpcQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let resp = self.list_collections_raw().await?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        let mut cl = self.collections_client();
        match cl
            .collection_exists(tonic::Request::new(qdrant::CollectionExistsRequest {
                collection_name: name.to_string(),
            }))
            .await
        {
            Ok(resp) => Ok(resp.into_inner().result.map(|r| r.exists).unwrap_or(false)),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(false),
            Err(e) => Err(QqlError::backend(
                "QQL-GRPC",
                format!("collection_exists: {e}"),
                None,
            )),
        }
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self.collection_info_raw(name.to_string()).await?;
        let info = resp
            .result
            .ok_or_else(|| QqlError::backend("QQL-GRPC", "collection_info: no result", None))?;

        Ok(CollectionInfo {
            status: info.status.to_string(),
            points_count: info.points_count.unwrap_or(0),
            segments_count: info.segments_count,
            schema: schema_from_grpc_collection(&info),
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        let vectors_config = req.vectors_config.and_then(|v| {
            let obj = v.as_object()?;
            let mut map = std::collections::HashMap::new();
            for (name, cfg) in obj {
                let size = cfg.get("size").and_then(|s| s.as_u64()).unwrap_or(384);
                let dist = cfg
                    .get("distance")
                    .and_then(|d| d.as_str())
                    .map(|d| match d {
                        "Cosine" => qdrant::Distance::Cosine as i32,
                        "Euclid" => qdrant::Distance::Euclid as i32,
                        "Dot" => qdrant::Distance::Dot as i32,
                        "Manhattan" => qdrant::Distance::Manhattan as i32,
                        _ => qdrant::Distance::Cosine as i32,
                    })
                    .unwrap_or(qdrant::Distance::Cosine as i32);
                map.insert(
                    name.clone(),
                    qdrant::VectorParams {
                        size,
                        distance: dist,
                        ..Default::default()
                    },
                );
            }
            Some(qdrant::VectorsConfig {
                config: Some(qdrant::vectors_config::Config::ParamsMap(
                    qdrant::VectorParamsMap { map },
                )),
            })
        });

        self.create_collection_raw(qdrant::CreateCollection {
            collection_name: req.collection_name,
            vectors_config,
            ..Default::default()
        })
        .await
        .map(|_| ())
    }

    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "update_collection: use execute_route for gRPC",
            None,
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.delete_collection_raw(qdrant::DeleteCollection {
            collection_name: name.to_string(),
            ..Default::default()
        })
        .await
        .map(|_| ())
    }

    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "create_field_index: use execute_route for gRPC",
            None,
        ))
    }

    async fn delete_field_index(
        &self,
        _collection_name: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "delete_field_index: use execute_route for gRPC",
            None,
        ))
    }

    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, QqlError> {
        crate::grpc_route::execute_planned_grpc(self, op).await
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        crate::grpc_route::execute_query_batch_grpc(self, collection, batch).await
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        crate::grpc_route::execute_update_batch_grpc(self, collection, batch).await
    }
}

/// Map gRPC collection info into the shared typed schema (no JSON round-trip).
fn schema_from_grpc_collection(info: &qdrant::CollectionInfo) -> CollectionSchema {
    let mut schema = CollectionSchema::default();

    if let Some(config) = &info.config {
        if let Some(params) = &config.params {
            if let Some(vc) = &params.vectors_config {
                match &vc.config {
                    Some(qdrant::vectors_config::Config::Params(p)) => {
                        schema.vectors.push(vector_params_to_spec(None, p));
                        schema.dense_vectors.clear();
                    }
                    Some(qdrant::vectors_config::Config::ParamsMap(map)) => {
                        let mut names: Vec<String> = map.map.keys().cloned().collect();
                        names.sort();
                        schema.dense_vectors = names.clone();
                        for name in &names {
                            if let Some(p) = map.map.get(name) {
                                schema
                                    .vectors
                                    .push(vector_params_to_spec(Some(name.clone()), p));
                            }
                        }
                    }
                    None => {}
                }
            }
            if let Some(sparse) = &params.sparse_vectors_config {
                let mut specs = Vec::new();
                for (name, p) in &sparse.map {
                    let modifier = p
                        .modifier
                        .and_then(|m| qdrant::Modifier::try_from(m).ok())
                        .map(|m| match m {
                            qdrant::Modifier::Idf => "idf".to_string(),
                            qdrant::Modifier::None => "none".to_string(),
                        });
                    let index = p.index.as_ref().map(|i| {
                        let mut map = serde_json::Map::new();
                        if let Some(v) = i.full_scan_threshold {
                            map.insert("full_scan_threshold".into(), serde_json::json!(v));
                        }
                        if let Some(v) = i.on_disk {
                            map.insert("on_disk".into(), serde_json::Value::Bool(v));
                        }
                        if let Some(v) = i.datatype {
                            if let Ok(dt) = qdrant::Datatype::try_from(v) {
                                // Normalize protobuf enum names to QQL/OpenAPI forms.
                                let name = match dt {
                                    qdrant::Datatype::Float32 => "float32",
                                    qdrant::Datatype::Uint8 => "uint8",
                                    qdrant::Datatype::Float16 => "float16",
                                    qdrant::Datatype::Default => "default",
                                };
                                map.insert(
                                    "datatype".into(),
                                    serde_json::Value::String(name.into()),
                                );
                            }
                        }
                        map
                    });
                    specs.push(SparseVectorSpec {
                        name: name.clone(),
                        index,
                        modifier,
                    });
                }
                specs.sort_by(|a, b| a.name.cmp(&b.name));
                schema.sparse_vectors = specs;
            }
            let sharding_method = params.sharding_method.and_then(|m| {
                qdrant::ShardingMethod::try_from(m).ok().map(|sm| match sm {
                    qdrant::ShardingMethod::Auto => "auto".to_string(),
                    qdrant::ShardingMethod::Custom => "custom".to_string(),
                })
            });
            schema.params = CollectionParamsSpec {
                shard_number: Some(params.shard_number as u64),
                sharding_method,
                on_disk_payload: Some(params.on_disk_payload),
                replication_factor: params.replication_factor.map(|n| n as u64),
            };
        }
        if let Some(hnsw) = &config.hnsw_config {
            schema.hnsw = Some(hnsw_diff_to_map(hnsw));
        }
        if let Some(opt) = &config.optimizer_config {
            schema.optimizers = Some(optimizer_config_diff_to_map(opt));
        }
        if let Some(quant) = &config.quantization_config {
            schema.quantization = quantization_config_to_json(quant);
        }
    }

    for (field, meta) in &info.payload_schema {
        let data_type = qdrant::PayloadSchemaType::try_from(meta.data_type)
            .ok()
            .map(|t| t.as_str_name().to_ascii_lowercase())
            .unwrap_or_else(|| "keyword".into());
        let data_type = match data_type.as_str() {
            "unknowntype" => "keyword".to_string(),
            other => other.to_string(),
        };

        let (params_map, is_tenant) = payload_index_params_from_proto(meta.params.as_ref());
        schema.payload_indexes.push(PayloadIndexSpec {
            field: field.clone(),
            data_type,
            params: params_map,
            is_tenant,
        });
    }
    schema.payload_indexes.sort_by(|a, b| a.field.cmp(&b.field));

    schema
}

fn vector_params_to_spec(name: Option<String>, p: &qdrant::VectorParams) -> VectorSpec {
    let distance = qdrant::Distance::try_from(p.distance)
        .ok()
        .map(|d| d.as_str_name().to_string())
        .unwrap_or_else(|| "Cosine".into());
    let hnsw = p.hnsw_config.as_ref().map(hnsw_diff_to_map);
    let quantization = p
        .quantization_config
        .as_ref()
        .and_then(quantization_config_to_json);
    let multivector = p.multivector_config.as_ref().map(|m| {
        let mut map = serde_json::Map::new();
        if let Ok(c) = qdrant::MultiVectorComparator::try_from(m.comparator) {
            let name = match c {
                qdrant::MultiVectorComparator::MaxSim => "max_sim",
            };
            map.insert("comparator".into(), serde_json::Value::String(name.into()));
        }
        map
    });
    VectorSpec {
        name,
        size: p.size,
        distance,
        hnsw,
        quantization,
        multivector,
        on_disk: p.on_disk,
    }
}

fn hnsw_diff_to_map(diff: &qdrant::HnswConfigDiff) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if let Some(v) = diff.m {
        map.insert("m".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.ef_construct {
        map.insert("ef_construct".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.full_scan_threshold {
        map.insert("full_scan_threshold".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.max_indexing_threads {
        map.insert("max_indexing_threads".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.on_disk {
        map.insert("on_disk".into(), serde_json::Value::Bool(v));
    }
    if let Some(v) = diff.payload_m {
        map.insert("payload_m".into(), serde_json::Value::from(v));
    }
    map
}

fn optimizer_config_diff_to_map(
    diff: &qdrant::OptimizersConfigDiff,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if let Some(v) = diff.deleted_threshold {
        map.insert("deleted_threshold".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.vacuum_min_vector_number {
        map.insert("vacuum_min_vector_number".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.default_segment_number {
        map.insert("default_segment_number".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.max_segment_size {
        map.insert("max_segment_size".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.indexing_threshold {
        map.insert("indexing_threshold".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.flush_interval_sec {
        map.insert("flush_interval_sec".into(), serde_json::json!(v));
    }
    if let Some(ref mot) = diff.max_optimization_threads {
        if let Some(ref variant) = mot.variant {
            match variant {
                qdrant::max_optimization_threads::Variant::Value(val) => {
                    map.insert("max_optimization_threads".into(), serde_json::json!(val));
                }
                qdrant::max_optimization_threads::Variant::Setting(_) => {
                    map.insert("max_optimization_threads".into(), serde_json::json!("auto"));
                }
            }
        }
    }
    map
}

/// Convert protobuf quantization into the nested REST-shaped JSON that dump
/// understands (`{ "scalar": {...} }`, `{ "turbo": {...} }`, …).
fn quantization_config_to_json(q: &qdrant::QuantizationConfig) -> Option<serde_json::Value> {
    use qdrant::quantization_config::Quantization;
    match &q.quantization {
        Some(Quantization::Scalar(sq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("scalar"));
            if let Some(v) = sq.quantile {
                map.insert("quantile".into(), serde_json::json!(v));
            }
            if let Some(v) = sq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            Some(serde_json::json!({ "scalar": map }))
        }
        Some(Quantization::Product(pq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("product"));
            if let Ok(c) = qdrant::CompressionRatio::try_from(pq.compression) {
                map.insert(
                    "compression".into(),
                    serde_json::Value::String(c.as_str_name().to_string()),
                );
            }
            if let Some(v) = pq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            Some(serde_json::json!({ "product": map }))
        }
        Some(Quantization::Binary(bq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("binary"));
            if let Some(v) = bq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            if let Some(enc) = bq.encoding {
                if let Ok(e) = qdrant::BinaryQuantizationEncoding::try_from(enc) {
                    // QQL CREATE accepts snake_case aliases, not protobuf enum names.
                    let qql_enc = match e {
                        qdrant::BinaryQuantizationEncoding::OneBit => "one_bit",
                        qdrant::BinaryQuantizationEncoding::TwoBits => "two_bits",
                        qdrant::BinaryQuantizationEncoding::OneAndHalfBits => "one_and_half_bits",
                    };
                    map.insert("encoding".into(), serde_json::Value::String(qql_enc.into()));
                }
            }
            if let Some(ref qe) = bq.query_encoding {
                if let Some(qdrant::binary_quantization_query_encoding::Variant::Setting(s)) =
                    qe.variant
                {
                    if let Ok(setting) =
                        qdrant::binary_quantization_query_encoding::Setting::try_from(s)
                    {
                        let name = match setting {
                            qdrant::binary_quantization_query_encoding::Setting::Binary => "binary",
                            qdrant::binary_quantization_query_encoding::Setting::Scalar4Bits => {
                                "scalar4bits"
                            }
                            qdrant::binary_quantization_query_encoding::Setting::Scalar8Bits => {
                                "scalar8bits"
                            }
                            qdrant::binary_quantization_query_encoding::Setting::Default => {
                                "default"
                            }
                        };
                        map.insert(
                            "query_encoding".into(),
                            serde_json::Value::String(name.into()),
                        );
                    }
                }
            }
            Some(serde_json::json!({ "binary": map }))
        }
        Some(Quantization::Turboquant(tq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("turbo"));
            if let Some(v) = tq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            if let Some(bits) = tq.bits {
                if let Ok(b) = qdrant::TurboQuantBitSize::try_from(bits) {
                    // Numeric form matches QQL CREATE: bits = 1 | 1.5 | 2 | 4
                    let n = match b {
                        qdrant::TurboQuantBitSize::Bits1 => 1.0,
                        qdrant::TurboQuantBitSize::Bits15 => 1.5,
                        qdrant::TurboQuantBitSize::Bits2 => 2.0,
                        qdrant::TurboQuantBitSize::Bits4 => 4.0,
                    };
                    map.insert("bits".into(), serde_json::json!(n));
                }
            }
            Some(serde_json::json!({ "turbo": map }))
        }
        None => None,
    }
}

/// Extract dump-relevant index options from protobuf params (best-effort).
fn payload_index_params_from_proto(
    params: Option<&qdrant::PayloadIndexParams>,
) -> (serde_json::Map<String, serde_json::Value>, Option<bool>) {
    let mut map = serde_json::Map::new();
    let mut is_tenant = None;
    let Some(params) = params else {
        return (map, is_tenant);
    };
    use qdrant::payload_index_params::IndexParams;
    match &params.index_params {
        Some(IndexParams::KeywordIndexParams(p)) => {
            is_tenant = p.is_tenant;
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::UuidIndexParams(p)) => {
            is_tenant = p.is_tenant;
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::TextIndexParams(p)) => {
            if let Some(v) = p.lowercase {
                map.insert("lowercase".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Ok(tok) = qdrant::TokenizerType::try_from(p.tokenizer) {
                let name = tok.as_str_name().to_ascii_lowercase();
                if name != "unknowntokenizer" {
                    map.insert("tokenizer".into(), serde_json::Value::String(name));
                }
            }
            if let Some(v) = p.min_token_len {
                map.insert("min_token_len".into(), serde_json::Value::from(v));
            }
            if let Some(v) = p.max_token_len {
                map.insert("max_token_len".into(), serde_json::Value::from(v));
            }
        }
        Some(IndexParams::IntegerIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.is_principal {
                map.insert("is_principal".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::FloatIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::DatetimeIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::GeoIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::BoolIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        None => {}
    }
    (map, is_tenant)
}
