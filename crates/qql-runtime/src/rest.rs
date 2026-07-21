use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use qql_core::error::QqlError;
use qql_plan::routing::Route;

use crate::client::{
    CollectionInfo, CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq,
    GetPointsReq, PointGroup, QdrantAdminOps, QdrantCoreOps, RetrievedPoint, ScoredPoint,
    ScrollPointsReq, SetPayloadReq, UpdateVectorsReq, UpsertPointsReq,
};
use crate::pipeline::{PointId, QueryPointsGroupsRequest, QueryPointsRequest};

/// REST implementation of [`QdrantOps`].
///
/// The client is reusable and therefore retains Reqwest's connection pool across
/// QQL statements. Applications that already own a `reqwest::Client` can inject
/// it with [`Self::with_client`].
#[derive(Clone)]
pub struct RestQdrant {
    base_url: String,
    api_key: Option<String>,
    client: Client,
}

impl RestQdrant {
    pub fn new(url: impl Into<String>, api_key: Option<String>) -> Result<Self, QqlError> {
        #[cfg(not(target_arch = "wasm32"))]
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("failed to build REST client: {error}"), None))?;

        #[cfg(target_arch = "wasm32")]
        let client = Client::builder()
            .build()
            .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("failed to build REST client: {error}"), None))?;

        Self::with_client(url, api_key, client)
    }

    pub fn with_client(
        url: impl Into<String>,
        api_key: Option<String>,
        client: Client,
    ) -> Result<Self, QqlError> {
        let base_url = url.into().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            return Err(QqlError::validation("QQL-RUNTIME", "Qdrant REST URL is required", None));
        }
        Ok(Self {
            base_url,
            api_key,
            client,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, QqlError> {
        let mut request = self.client.request(method, self.url(path));
        if let Some(api_key) = &self.api_key {
            request = request.header("api-key", api_key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request
            .send()
            .await
            .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("Qdrant REST request failed: {error}"), None))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            QqlError::validation("QQL-RUNTIME", format!("failed to read Qdrant REST response: {error}"), None)
        })?;
        if !status.is_success() {
            let detail: String = body.chars().take(4_096).collect();
            return Err(QqlError::validation("QQL-RUNTIME", format!(
                "Qdrant REST request returned {status}: {detail}"
            ), None));
        }

        let envelope: Value = serde_json::from_str(&body)
            .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("invalid Qdrant REST response: {error}"), None))?;
        let result = envelope.get("result").cloned().unwrap_or(envelope);
        serde_json::from_value(result)
            .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("unexpected Qdrant REST response: {error}"), None))
    }
}

pub(crate) fn point_id_json(id: PointId) -> Value {
    match id {
        PointId::Num(value) => json!(value),
        PointId::Uuid(value) => json!(value),
    }
}

fn points_result(value: Value) -> Result<Vec<ScoredPoint>, QqlError> {
    let points = value.get("points").cloned().unwrap_or(value);
    serde_json::from_value(points)
        .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("invalid Qdrant query result: {error}"), None))
}

fn remove_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for v in map.values_mut() {
                remove_nulls(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                remove_nulls(v);
            }
        }
        _ => {}
    }
}

pub(crate) fn query_request_json(request: &QueryPointsRequest) -> Result<Value, QqlError> {
    let mut body = serde_json::to_value(request).map_err(|error| {
        QqlError::validation("QQL-RUNTIME", format!("failed to serialize query request: {error}"), None)
    })?;
    remove_nulls(&mut body);
    Ok(body)
}

pub(crate) fn grouped_query_request_json(
    request: &QueryPointsGroupsRequest,
) -> Result<Value, QqlError> {
    let mut body = serde_json::to_value(request).map_err(|error| {
        QqlError::validation("QQL-RUNTIME", format!(
            "failed to serialize grouped query request: {error}"
        ), None)
    })?;
    remove_nulls(&mut body);
    Ok(body)
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl QdrantCoreOps for RestQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let value: Value = self.call(Method::GET, "/collections", None).await?;
        Ok(value
            .get("collections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|collection| collection.get("name").and_then(Value::as_str))
            .map(str::to_owned)
            .collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        let value: Value = self
            .call(Method::GET, &format!("/collections/{name}/exists"), None)
            .await?;
        value
            .get("exists")
            .and_then(Value::as_bool)
            .ok_or_else(|| QqlError::validation("QQL-RUNTIME", "Qdrant did not return collection existence", None))
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let value: Value = self
            .call(Method::GET, &format!("/collections/{name}"), None)
            .await?;
        let vectors = value
            .pointer("/config/params/vectors")
            .and_then(Value::as_object);
        let dense_vectors = vectors
            .map(|vectors| {
                if vectors.contains_key("size") {
                    vec![String::new()]
                } else {
                    vectors.keys().cloned().collect()
                }
            })
            .unwrap_or_default();
        let sparse_vectors = value
            .pointer("/config/params/sparse_vectors")
            .and_then(Value::as_object)
            .map(|vectors| vectors.keys().cloned().collect())
            .unwrap_or_default();
        Ok(CollectionInfo {
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            points_count: value
                .get("points_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            segments_count: value
                .get("segments_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            schema: crate::backend::CollectionSchema {
                dense_vectors,
                sparse_vectors,
            },
            raw_json: Some(value.clone()),
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        self.call::<Value>(
            Method::PUT,
            &format!("/collections/{}?wait=true", req.collection_name),
            Some(json!({
                "vectors": req.vectors_config,
                "sparse_vectors": req.sparse_vectors_config,
                "hnsw_config": req.hnsw_config,
                "optimizers_config": req.optimizers_config,
                "quantization_config": req.quantization_config,
                "wal_config": Value::Null,
                "shard_number": Value::Null,
                "replication_factor": Value::Null,
                "write_consistency_factor": Value::Null,
                "on_disk_payload": Value::Null,
                "init_from": Value::Null,
                "params": req.params,
            })),
        )
        .await?;
        Ok(())
    }

    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        let points: Result<Vec<Value>, QqlError> = req
            .points
            .into_iter()
            .map(|point| {
                Ok(json!({
                    "id": point_id_json(point.id),
                    "vector": point.vector,
                    "payload": point.payload,
                }))
            })
            .collect();
        self.call::<Value>(
            Method::PUT,
            &format!("/collections/{}/points?wait=true", req.collection_name),
            Some(json!({ "points": points? })),
        )
        .await?;
        Ok(())
    }

    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError> {
        let collection = req.collection_name.clone();
        let value = self
            .call(
                Method::POST,
                &format!("/collections/{collection}/points/query"),
                Some(query_request_json(&req)?),
            )
            .await?;
        points_result(value)
    }

    async fn query_groups(
        &self,
        req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        let collection = req.collection_name.clone();
        let value: Value = self
            .call(
                Method::POST,
                &format!("/collections/{collection}/points/query/groups"),
                Some(grouped_query_request_json(&req)?),
            )
            .await?;
        let groups = value.get("groups").cloned().unwrap_or(value);
        serde_json::from_value(groups).map_err(|error| QqlError::validation("QQL-RUNTIME", error.to_string(), None))
    }

    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError> {
        let selector = if let Some(id) = req.point_id {
            json!({ "points": [point_id_json(id)] })
        } else {
            json!({ "filter": req.filter })
        };
        self.call::<Value>(
            Method::POST,
            &format!(
                "/collections/{}/points/delete?wait=true",
                req.collection_name
            ),
            Some(selector),
        )
        .await?;
        Ok(())
    }

    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError> {
        let vector = match req.vector_name {
            Some(name) => json!({ name: req.vector }),
            None => json!(req.vector),
        };
        self.call::<Value>(
            Method::PUT,
            &format!(
                "/collections/{}/points/vectors?wait=true",
                req.collection_name
            ),
            Some(json!({
                "points": [{ "id": point_id_json(req.point_id), "vector": vector }]
            })),
        )
        .await?;
        Ok(())
    }

    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError> {
        let selector = if let Some(id) = req.point_id {
            json!({ "points": [point_id_json(id)] })
        } else {
            json!({ "filter": req.filter })
        };
        let mut body = selector;
        body["payload"] = serde_json::to_value(req.payload)
            .map_err(|error| QqlError::validation("QQL-RUNTIME", error.to_string(), None))?;
        self.call::<Value>(
            Method::POST,
            &format!(
                "/collections/{}/points/payload?wait=true",
                req.collection_name
            ),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError> {
        let value: Value = self
            .call(
                Method::POST,
                &format!("/collections/{}/points/scroll", req.collection_name),
                Some(json!({
                    "limit": req.limit,
                    "filter": req.filter,
                    "offset": req.after.map(point_id_json),
                    "with_payload": true,
                    "with_vector": true,
                })),
            )
            .await?;
        let points = value.get("points").cloned().unwrap_or_else(|| json!([]));
        let points =
            serde_json::from_value(points).map_err(|error| QqlError::validation("QQL-RUNTIME", error.to_string(), None))?;
        let offset = match value.get("next_page_offset") {
            Some(v) if !v.is_null() => serde_json::from_value(v.clone())
                .map(Some)
                .map_err(|error| QqlError::validation("QQL-RUNTIME", format!("invalid scroll offset: {error}"), None))?,
            _ => None,
        };
        Ok((points, offset))
    }

    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        let id = crate::executor::helpers::value_to_json(&req.point_id);
        let points: Value = self
            .call(
                Method::POST,
                &format!("/collections/{}/points", req.collection_name),
                Some(json!({ "ids": [id], "with_payload": true })),
            )
            .await?;
        serde_json::from_value(points).map_err(|error| QqlError::validation("QQL-RUNTIME", error.to_string(), None))
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl QdrantAdminOps for RestQdrant {
    async fn update_collection(&self, req: Value) -> Result<(), QqlError> {
        let name = req
            .get("collection_name")
            .and_then(Value::as_str)
            .ok_or_else(|| QqlError::validation("QQL-RUNTIME", "collection_name is required", None))?
            .to_owned();
        let mut body = req;
        body.as_object_mut()
            .expect("request is constructed as an object")
            .remove("collection_name");
        self.call::<Value>(
            Method::PATCH,
            &format!("/collections/{name}?wait=true"),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.call::<Value>(
            Method::DELETE,
            &format!("/collections/{name}?wait=true"),
            None,
        )
        .await?;
        Ok(())
    }

    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        let futures = req.into_iter().map(|query| self.query(query));
        futures_util::future::try_join_all(futures).await
    }

    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError> {
        let options: HashMap<String, Value> = req
            .options
            .into_iter()
            .map(|(key, value)| (key, crate::executor::helpers::value_to_json(&value)))
            .collect();
        self.call::<Value>(
            Method::PUT,
            &format!("/collections/{}/index?wait=true", req.collection_name),
            Some(json!({
                "field_name": req.field,
                "field_schema": req.field_type,
                "field_index_params": options,
            })),
        )
        .await?;
        Ok(())
    }

    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError> {
        let value: Value = self
            .call(
                Method::POST,
                &format!("/collections/{}/points/count", req.collection_name),
                Some(json!({ "filter": req.filter, "exact": true })),
            )
            .await?;
        value
            .get("count")
            .and_then(Value::as_u64)
            .ok_or_else(|| QqlError::validation("QQL-RUNTIME", "Qdrant did not return a point count", None))
    }

    async fn execute_route(&self, route: Route) -> Result<Value, QqlError> {
        let method = match route.method {
            qql_plan::types::Method::Get => Method::GET,
            qql_plan::types::Method::Post => Method::POST,
            qql_plan::types::Method::Put => Method::PUT,
            qql_plan::types::Method::Patch => Method::PATCH,
            qql_plan::types::Method::Delete => Method::DELETE,
        };

        let mut path = route.path.clone();
        if !route.query.is_empty() {
            let qs: Vec<String> = route
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            path.push('?');
            path.push_str(&qs.join("&"));
        }

        let body: Option<Value> = route.body.as_ref().map(|b| b.to_json());

        let result: Value = self.call(method, &path, body).await?;
        Ok(result)
    }
}
