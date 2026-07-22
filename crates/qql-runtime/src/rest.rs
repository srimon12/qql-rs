use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::Value;

use qql_core::error::QqlError;
use qql_plan::routing::Route;
use qql_plan::types::Method as PlanMethod;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::backend::CollectionSchema;
use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};

#[derive(Clone)]
pub struct RestQdrant {
    base_url: String,
    api_key: Option<String>,
    client: Client,
}

impl RestQdrant {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let base_url = base_url.into();
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client,
        }
    }

    pub fn with_client(base_url: String, api_key: Option<String>, client: Client) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client,
        }
    }

    async fn call_body<B: serde::Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, QqlError> {
        let mut url_buf = String::with_capacity(self.base_url.len() + path.len());
        url_buf.push_str(&self.base_url);
        url_buf.push_str(path);
        let mut req = self.client.request(method, &url_buf);
        if let Some(ref key) = self.api_key {
            req = req.header("api-key", key);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT",
                format!("HTTP request failed: {error}"),
                None,
            )
        })?;
        let status = resp.status();
        let text = resp.text().await.map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND",
                format!("failed to read response body: {error}"),
                None,
            )
        })?;
        if !status.is_success() {
            let detail = if text.len() > 4096 {
                &text[..4096]
            } else {
                &text
            };
            return Err(QqlError::backend(
                "QQL-BACKEND",
                format!("Qdrant returned {status}: {detail}"),
                None,
            ));
        }
        serde_json::from_str(&text).map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND",
                format!("failed to parse Qdrant response: {error}"),
                None,
            )
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, QqlError> {
        self.call_body(method, path, body.as_ref()).await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl QdrantOps for RestQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let value: Value = self.call(Method::GET, "/collections", None).await?;
        let collections = value
            .get("result")
            .and_then(|r| r.get("collections"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(collections
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::as_str).map(String::from))
            .collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        match self
            .call::<Value>(Method::GET, &format!("/collections/{name}"), None)
            .await
        {
            Ok(value) => Ok(value
                .get("result")
                .and_then(|r| r.get("status").or_else(|| r.get("exists")))
                .map(|_| true)
                .unwrap_or(true)),
            Err(e) if e.message.contains("404") || e.message.contains("Not found") => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let value: Value = self
            .call(Method::GET, &format!("/collections/{name}"), None)
            .await?;
        let result = value.get("result").cloned().unwrap_or(value);

        // Extract vector names from the raw Qdrant response.  Dense vectors
        // are keys under config.params.vectors, sparse under
        // config.params.sparse_vectors.
        let mut schema = CollectionSchema::default();
        if let Some(vectors) = result
            .get("config")
            .and_then(|c| c.get("params"))
            .and_then(|p| p.get("vectors"))
            .and_then(|v| v.as_object())
        {
            schema.dense_vectors = vectors.keys().cloned().collect();
        }
        if let Some(sparse) = result
            .get("config")
            .and_then(|c| c.get("params"))
            .and_then(|p| p.get("sparse_vectors"))
            .and_then(|v| v.as_object())
        {
            schema.sparse_vectors = sparse.keys().cloned().collect();
        }

        let mut info: CollectionInfo = serde_json::from_value(result).map_err(|e| {
            QqlError::backend("QQL-BACKEND", format!("parse collection info: {e}"), None)
        })?;
        info.schema = schema;
        Ok(info)
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        let mut body = serde_json::Map::new();
        if let Some(v) = &req.vectors_config {
            body.insert("vectors".into(), v.clone());
        }
        if let Some(v) = &req.sparse_vectors_config {
            body.insert("sparse_vectors".into(), v.clone());
        }
        if let Some(v) = req.shard_number {
            body.insert("shard_number".into(), serde_json::Value::from(v));
        }
        if let Some(ref v) = req.sharding_method {
            body.insert(
                "sharding_method".into(),
                serde_json::Value::String(v.clone()),
            );
        }
        if let Some(ref v) = req.shard_keys {
            body.insert(
                "shard_keys".into(),
                serde_json::Value::Array(
                    v.iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        // replication_factor and write_consistency_factor are sent as top-level
        // fields in Qdrant REST API (not nested inside params)
        if let Some(ref p) = req.params {
            if let Some(rf) = p.get("replication_factor").and_then(|v| v.as_u64()) {
                body.insert("replication_factor".into(), serde_json::Value::from(rf));
            }
            if let Some(wc) = p.get("write_consistency_factor").and_then(|v| v.as_u64()) {
                body.insert(
                    "write_consistency_factor".into(),
                    serde_json::Value::from(wc),
                );
            }
            if let Some(od) = p.get("on_disk_payload").and_then(|v| v.as_bool()) {
                body.insert("on_disk_payload".into(), serde_json::Value::Bool(od));
            }
        }
        // hnsw_config, optimizers_config, quantization_config
        if let Some(ref v) = req.hnsw_config {
            body.insert("hnsw_config".into(), v.clone());
        }
        if let Some(ref v) = req.optimizers_config {
            body.insert("optimizers_config".into(), v.clone());
        }
        if let Some(ref v) = req.quantization_config {
            body.insert("quantization_config".into(), v.clone());
        }
        self.call::<Value>(
            Method::PUT,
            &format!("/collections/{}", req.collection_name),
            Some(Value::Object(body)),
        )
        .await?;
        Ok(())
    }

    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError> {
        let collection_name = req
            .get("collection_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                QqlError::execution("QQL-EXECUTION", "collection_name required", None)
            })?;
        let mut body = req.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.remove("collection_name");
        }
        self.call::<Value>(
            Method::PATCH,
            &format!("/collections/{collection_name}"),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.call::<Value>(Method::DELETE, &format!("/collections/{name}"), None)
            .await?;
        Ok(())
    }

    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError> {
        let mut body = serde_json::json!({
            "field_name": req.field,
            "field_schema": req.field_type,
        });
        if let Some(obj) = body.as_object_mut() {
            for (k, v) in req.options {
                obj.insert(k, crate::executor::helpers::value_to_json(&v));
            }
        }
        self.call::<Value>(
            Method::PUT,
            &format!("/collections/{}/index", req.collection_name),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn delete_field_index(
        &self,
        collection_name: &str,
        field_name: &str,
    ) -> Result<(), QqlError> {
        self.call::<Value>(
            Method::DELETE,
            &format!("/collections/{}/index/{}", collection_name, field_name),
            None::<Value>,
        )
        .await?;
        Ok(())
    }

    async fn execute_route(&self, route: Route) -> Result<Value, QqlError> {
        let method = match route.method {
            PlanMethod::Get => Method::GET,
            PlanMethod::Post => Method::POST,
            PlanMethod::Put => Method::PUT,
            PlanMethod::Patch => Method::PATCH,
            PlanMethod::Delete => Method::DELETE,
        };

        let mut path = String::with_capacity(route.path.len() + 32);
        path.push_str(&route.path);
        if !route.query.is_empty() {
            path.push('?');
            for (i, (k, v)) in route.query.iter().enumerate() {
                if i > 0 {
                    path.push('&');
                }
                path.push_str(k);
                path.push('=');
                path.push_str(v);
            }
        }

        self.call_body(method, &path, route.body.as_ref()).await
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<Value>, QqlError> {
        let path = format!("/collections/{collection}/points/query/batch");
        let value: Value = self.call_body(Method::POST, &path, Some(batch)).await?;
        Ok(value
            .get("result")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default())
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<Value>, QqlError> {
        let path = format!("/collections/{collection}/points/batch?wait=true");
        let value: Value = self.call_body(Method::POST, &path, Some(batch)).await?;
        Ok(value
            .get("result")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default())
    }
}
