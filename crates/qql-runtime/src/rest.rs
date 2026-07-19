use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use qql_core::error::QqlError;

use crate::client::{
    CollectionInfo, CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq,
    GetPointsReq, PointGroup, QdrantOps, RetrievedPoint, ScoredPoint, ScrollPointsReq,
    SetPayloadReq, UpdateVectorsReq, UpsertPointsReq,
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
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| QqlError::runtime(format!("failed to build REST client: {error}")))?;
        Self::with_client(url, api_key, client)
    }

    pub fn with_client(
        url: impl Into<String>,
        api_key: Option<String>,
        client: Client,
    ) -> Result<Self, QqlError> {
        let base_url = url.into().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            return Err(QqlError::runtime("Qdrant REST URL is required"));
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
            .map_err(|error| QqlError::runtime(format!("Qdrant REST request failed: {error}")))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            QqlError::runtime(format!("failed to read Qdrant REST response: {error}"))
        })?;
        if !status.is_success() {
            let detail: String = body.chars().take(4_096).collect();
            return Err(QqlError::runtime(format!(
                "Qdrant REST request returned {status}: {detail}"
            )));
        }

        let envelope: Value = serde_json::from_str(&body)
            .map_err(|error| QqlError::runtime(format!("invalid Qdrant REST response: {error}")))?;
        let result = envelope.get("result").cloned().unwrap_or(envelope);
        serde_json::from_value(result)
            .map_err(|error| QqlError::runtime(format!("unexpected Qdrant REST response: {error}")))
    }
}

fn point_id_json(id: PointId) -> Value {
    match id {
        PointId::Num(value) => json!(value),
        PointId::Uuid(value) => json!(value),
    }
}

fn points_result(value: Value) -> Result<Vec<ScoredPoint>, QqlError> {
    let mut points = value.get("points").cloned().unwrap_or(value);
    normalize_point_ids(&mut points);
    serde_json::from_value(points)
        .map_err(|error| QqlError::runtime(format!("invalid Qdrant query result: {error}")))
}

/// The generated OpenAPI types model point IDs as objects, while Qdrant's REST
/// API returns primitive JSON values. Normalize only at this adapter boundary.
fn normalize_point_ids(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_point_ids),
        Value::Object(object) => {
            for key in ["id", "next_page_offset"] {
                if let Some(id) = object.get_mut(key) {
                    let normalized = match id {
                        Value::Number(number) => Some(json!({ "num": number, "uuid": null })),
                        Value::String(uuid) => Some(json!({ "num": null, "uuid": uuid })),
                        _ => None,
                    };
                    if let Some(normalized) = normalized {
                        *id = normalized;
                    }
                }
            }
            object.values_mut().for_each(normalize_point_ids);
        }
        _ => {}
    }
}

#[async_trait]
impl QdrantOps for RestQdrant {
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
            .ok_or_else(|| QqlError::runtime("Qdrant did not return collection existence"))
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        self.call(Method::GET, &format!("/collections/{name}"), None)
            .await
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

    async fn update_collection(&self, req: Value) -> Result<(), QqlError> {
        let name = req
            .get("collection_name")
            .and_then(Value::as_str)
            .ok_or_else(|| QqlError::runtime("collection_name is required"))?
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

    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        let points: Result<Vec<Value>, QqlError> = req
            .points
            .into_iter()
            .map(|point| {
                Ok(json!({
                    "id": point_id_json(point.id.into()),
                    "vector": serde_json::to_value(point.vector)
                        .map_err(|error| QqlError::runtime(error.to_string()))?,
                    "payload": serde_json::to_value(point.payload)
                        .map_err(|error| QqlError::runtime(error.to_string()))?,
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
                Some(
                    serde_json::to_value(req)
                        .map_err(|error| QqlError::runtime(error.to_string()))?,
                ),
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
                Some(
                    serde_json::to_value(req)
                        .map_err(|error| QqlError::runtime(error.to_string()))?,
                ),
            )
            .await?;
        let mut groups = value.get("groups").cloned().unwrap_or(value);
        normalize_point_ids(&mut groups);
        serde_json::from_value(groups).map_err(|error| QqlError::runtime(error.to_string()))
    }

    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        let mut results = Vec::with_capacity(req.len());
        for query in req {
            results.push(self.query(query).await?);
        }
        Ok(results)
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
            .map_err(|error| QqlError::runtime(error.to_string()))?;
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
                })),
            )
            .await?;
        let mut points = value.get("points").cloned().unwrap_or_else(|| json!([]));
        normalize_point_ids(&mut points);
        let points =
            serde_json::from_value(points).map_err(|error| QqlError::runtime(error.to_string()))?;
        let mut offset = value.get("next_page_offset").cloned();
        if let Some(offset) = &mut offset {
            normalize_point_ids(offset);
        }
        let offset = offset
            .and_then(|value| serde_json::from_value::<crate::qdrant::ExtendedPointId>(value).ok())
            .map(Into::into);
        Ok((points, offset))
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
            .ok_or_else(|| QqlError::runtime("Qdrant did not return a point count"))
    }

    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        let id = crate::executor::helpers::to_point_id_static(&req.point_id)?;
        let mut points: Value = self
            .call(
                Method::POST,
                &format!("/collections/{}/points", req.collection_name),
                Some(json!({ "ids": [point_id_json(id)], "with_payload": true })),
            )
            .await?;
        normalize_point_ids(&mut points);
        serde_json::from_value(points).map_err(|error| QqlError::runtime(error.to_string()))
    }
}
