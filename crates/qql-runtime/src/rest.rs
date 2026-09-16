use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use qql_core::error::QqlError;
use qql_plan::types::Method as PlanMethod;
use qql_plan::types::ReadConsistencyParam;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

pub use crate::client::REQUEST_ID_HEADER;
pub(crate) use crate::client::next_request_id;
use crate::client::{CollectionInfo, QdrantOps};
use crate::executor::response::{BackendResponse, ExecData};

pub use super::rest_client::{ROUTE_AFFINITY_HEADER, RestQdrant};

/// Typed Qdrant success envelope (`{"result": …, "status": "ok", …}`) for the
/// [`RestQdrant::execute_typed`] fast path. Unknown fields (notably `time` /
/// `usage` telemetry) are ignored — telemetry stays a lenient separate
/// extraction on the paths that consume it.
#[derive(Debug, Deserialize)]
struct Envelope<T> {
    result: T,
    status: String,
}

pub(crate) fn classify_backend_error_code(status: u16, body: &str) -> &'static str {
    let lower = body.to_ascii_lowercase();
    if lower.contains("strict-mode")
        || lower.contains("strict mode")
        || lower.contains("quota exceeded")
    {
        "QQL-BACKEND-STRICT-MODE"
    } else if status == 401
        || (status == 403 && !lower.contains("strict"))
        || lower.contains("forbidden")
        || lower.contains("unauthorized")
        || lower.contains("api-key")
    {
        "QQL-BACKEND-AUTH"
    } else if status == 404 || lower.contains("not found") {
        "QQL-BACKEND-COLLECTION-NOT-FOUND"
    } else if (lower.contains("index")
        && (lower.contains("not exist")
            || lower.contains("appropriate")
            || lower.contains("not ready")
            || lower.contains("missing")
            || lower.contains("indexing")
            || lower.contains("failed")))
        || lower.contains("no appropriate index")
    {
        "QQL-BACKEND-INDEX-NOT-READY"
    } else if lower.contains("dimension")
        || lower.contains("vector size")
        || lower.contains("dimensions")
    {
        "QQL-BACKEND-DIMENSION-MISMATCH"
    } else {
        "QQL-BACKEND-HTTP"
    }
}

/// Whether a `GET /collections/{name}` failure means "collection missing".
///
/// This is the predicate behind `collection_exists`'s `Ok(false)` arm: a 404
/// status echoed in the message, or a "Not found" body. It is deliberately
/// narrower than [`classify_backend_error_code`]'s lowercase `"not found"`
/// match (see RT-09): a 200-envelope probe mentioning "index not found" must
/// not read as collection-missing here.
pub(crate) fn is_collection_missing_message(message: &str) -> bool {
    message.contains("404") || message.contains("Not found")
}

impl RestQdrant {
    fn apply_headers(
        &self,
        mut req: reqwest::RequestBuilder,
        request_id: &str,
    ) -> reqwest::RequestBuilder {
        // Pre-parsed values skip the per-request `&str` → `HeaderValue`
        // validation; the `None` arms behave exactly as before.
        if let Some(ref header) = self.api_key_header {
            req = req.header("api-key", header.clone());
        } else if let Some(ref key) = self.api_key {
            req = req.header("api-key", key);
        }
        if let Some(ref header) = self.affinity_header {
            req = req.header(ROUTE_AFFINITY_HEADER, header.clone());
        } else if let Some(ref affinity) = self.route_affinity {
            req = req.header(ROUTE_AFFINITY_HEADER, affinity);
        }
        req.header(REQUEST_ID_HEADER, request_id)
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
        let request_id = next_request_id();
        let mut req = self.client.request(method, &url_buf);
        req = self.apply_headers(req, &request_id);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT-REQUEST",
                format!("HTTP request failed: {error} (request id: {request_id})"),
                None,
            )
            .with_url(url_buf.clone())
            .with_field("request_id", request_id.clone())
        })?;
        let status = resp.status();
        let server_request_id = resp
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| request_id.clone());
        let text = resp.text().await.map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND-READ",
                format!("failed to read response body: {error}"),
                None,
            )
            .with_url(url_buf.clone())
        })?;
        if !status.is_success() {
            let limit = text.floor_char_boundary(4096);
            let detail = &text[..limit];
            let code = classify_backend_error_code(status.as_u16(), detail);
            return Err(QqlError::backend(
                code,
                format!("Qdrant returned {status}: {detail} (request id: {server_request_id})"),
                None,
            )
            .with_status(status.as_u16())
            .with_url(url_buf.clone())
            .with_field("request_id", server_request_id.clone()));
        }
        let value: Value = serde_json::from_str(&text).map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND-JSON",
                format!(
                    "failed to parse Qdrant response: {error} (request id: {server_request_id})"
                ),
                None,
            )
            .with_url(url_buf.clone())
        })?;
        validate_success_envelope(&value, path)?;
        serde_json::from_value(value).map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND-JSON",
                format!(
                    "failed to decode Qdrant response: {error} (request id: {server_request_id})"
                ),
                None,
            )
            .with_url(url_buf.clone())
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

    /// GET a JSON route and return the validated success-envelope value.
    pub(crate) async fn get_value(&self, path: &str) -> Result<Value, QqlError> {
        self.call::<Value>(Method::GET, path, None).await
    }

    /// Typed request/response fast path: a single `to_string` for the body
    /// and a single `from_str` for the envelope. `to_rest_route` stays for
    /// debugging and for paths that consume the raw envelope value.
    ///
    /// Error taxonomy matches [`Self::call_body`] verbatim: transport and
    /// JSON-syntax failures keep their messages, and valid JSON with an
    /// unexpected envelope shape reproduces
    /// [`validate_success_envelope`]'s messages (via an error-path-only
    /// re-parse — the hot path pays exactly one `from_str`).
    async fn execute_typed<B: serde::Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<T, QqlError> {
        let mut url_buf = String::with_capacity(self.base_url.len() + path.len());
        url_buf.push_str(&self.base_url);
        url_buf.push_str(path);
        let request_id = next_request_id();
        let mut req = self.client.request(method, &url_buf);
        if !query.is_empty() {
            req = req.query(&query);
        }
        req = self.apply_headers(req, &request_id);
        if let Some(b) = body {
            // Single serialization: `req.json` would do the same work, but
            // spelling it out keeps the to_string/from_str pairing visible.
            let text = serde_json::to_string(b).map_err(|error| {
                QqlError::execution(
                    "QQL-PLAN-SERIALIZE",
                    format!("plan IR REST request body serialization failed: {error}"),
                    None,
                )
            })?;
            req = req
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(text);
        }
        let resp = req.send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT-REQUEST",
                format!("HTTP request failed: {error} (request id: {request_id})"),
                None,
            )
            .with_url(url_buf.clone())
            .with_field("request_id", request_id.clone())
        })?;
        let status = resp.status();
        let server_request_id = resp
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| request_id.clone());
        let text = resp.text().await.map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND-READ",
                format!("failed to read response body: {error}"),
                None,
            )
            .with_url(url_buf.clone())
        })?;
        if !status.is_success() {
            let limit = text.floor_char_boundary(4096);
            let detail = &text[..limit];
            let code = classify_backend_error_code(status.as_u16(), detail);
            return Err(QqlError::backend(
                code,
                format!("Qdrant returned {status}: {detail} (request id: {server_request_id})"),
                None,
            )
            .with_status(status.as_u16())
            .with_url(url_buf.clone())
            .with_field("request_id", server_request_id.clone()));
        }
        let envelope: Envelope<T> = serde_json::from_str(&text).map_err(|error| {
            if error.is_data()
                && let Ok(shape) = serde_json::from_str::<Value>(&text)
                && let Err(shape_err) = validate_success_envelope(&shape, path)
            {
                return shape_err;
            }
            QqlError::backend(
                "QQL-BACKEND-JSON",
                format!(
                    "failed to parse Qdrant response: {error} (request id: {server_request_id})"
                ),
                None,
            )
            .with_url(url_buf.clone())
        })?;
        if envelope.status != "ok" {
            return Err(QqlError::backend(
                "QQL-BACKEND-ENVELOPE",
                format!("{path} response is missing status=ok"),
                None,
            ));
        }
        Ok(envelope.result)
    }

    /// Begin a streaming GET for an opaque, non-JSON body (a snapshot archive).
    ///
    /// Applies the same API-key / affinity / request-id headers and backend
    /// error classification as [`Self::call_body`], but leaves the body
    /// unread and unparsed so callers consume it chunk by chunk.
    pub(crate) async fn get_stream(&self, path: &str) -> Result<reqwest::Response, QqlError> {
        let url = format!("{}{}", self.base_url, path);
        let request_id = next_request_id();
        let request = self.apply_headers(self.client.get(&url), &request_id);
        let response = request.send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT-REQUEST",
                format!("HTTP request failed: {error} (request id: {request_id})"),
                None,
            )
            .with_url(url.clone())
            .with_field("request_id", request_id.clone())
        })?;
        let status = response.status();
        if !status.is_success() {
            let server_request_id = response
                .headers()
                .get(REQUEST_ID_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
                .unwrap_or_else(|| request_id.clone());
            let body = response.text().await.unwrap_or_default();
            let limit = body.floor_char_boundary(4096);
            let detail = &body[..limit];
            let code = classify_backend_error_code(status.as_u16(), detail);
            return Err(QqlError::backend(
                code,
                format!("Qdrant returned {status}: {detail} (request id: {server_request_id})"),
                None,
            )
            .with_status(status.as_u16())
            .with_url(url)
            .with_field("request_id", server_request_id));
        }
        Ok(response)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl QdrantOps for RestQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let value: Value = self.call(Method::GET, "/collections", None).await?;
        crate::rest_response::parse_collection_names(value)
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        match self
            .call::<Value>(Method::GET, &format!("/collections/{name}"), None)
            .await
        {
            Ok(value) => {
                validate_success_envelope(&value, "collection_exists")?;
                let status_ok = value
                    .get("result")
                    .and_then(|r| r.get("status").or_else(|| r.get("exists")))
                    .is_some();
                Ok(status_ok)
            }
            Err(e) if is_collection_missing_message(&e.message) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let value: Value = self
            .call(Method::GET, &format!("/collections/{name}"), None)
            .await?;
        crate::rest_response::parse_collection_info(value)
            .map_err(|error| error.with_collection(name.to_string()))
    }

    async fn create_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        self.create_collection_planned(collection_name, req).await
    }

    async fn update_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        let op = qql_plan::ddl::update_collection_op(collection_name, req);
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        // Status-only: skip the result subtree instead of building a `Value`
        // DOM that is discarded after validation.
        let _: serde::de::IgnoredAny = self
            .execute_typed(
                Method::DELETE,
                &format!("/collections/{name}"),
                &[],
                None::<&()>,
            )
            .await?;
        Ok(())
    }

    async fn create_field_index(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError> {
        let op = qql_plan::ddl::create_index_op(collection_name, req);
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn delete_field_index(
        &self,
        collection_name: &str,
        field_name: &str,
    ) -> Result<(), QqlError> {
        let op = qql_plan::ddl::drop_index_op(collection_name, field_name);
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<BackendResponse, QqlError> {
        if let qql_plan::PlannedOperation::CreateCollection {
            collection,
            request,
        } = op
        {
            self.create_collection_planned(collection, request).await?;
            return Ok(BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            });
        }
        let route = qql_plan::plan::to_rest_route(op).map_err(|err| match err {
            qql_plan::RestProjectionError::ClientSideOnly { stmt_type } => QqlError::execution(
                "QQL-REST-CLIENT-SIDE",
                format!("{stmt_type} cannot be executed as a single REST route"),
                None,
            ),
            qql_plan::RestProjectionError::SerializeFailed { message } => QqlError::execution(
                "QQL-PLAN-SERIALIZE",
                format!("plan IR REST request body serialization failed: {message}"),
                None,
            ),
            qql_plan::RestProjectionError::OverwriteRequiresBatch => QqlError::validation(
                "QQL-REST-OVERWRITE-BATCH-ONLY",
                "OVERWRITE has no single Qdrant REST route: POST /points/payload is merge-only;                  run the statement inside a BATCH block (POST /points/batch with overwrite_payload)",
                None,
            ),
        })?;
        let envelope = self.execute_http(route).await?;
        crate::rest_response::parse_planned(op, envelope)
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
        timeout: Option<u64>,
        consistency: Option<ReadConsistencyParam>,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        let path = format!("/collections/{collection}/points/query/batch");
        // Stack query pairs; `execute_typed` encodes them via reqwest.
        let timeout_value;
        let consistency_value;
        let mut query = Vec::new();
        if let Some(secs) = timeout {
            timeout_value = secs.to_string();
            query.push(("timeout", timeout_value.as_str()));
        }
        if let Some(consistency) = consistency.as_ref() {
            consistency_value = consistency.to_query_value();
            query.push(("consistency", consistency_value.as_str()));
        }
        let result: Value = self
            .execute_typed(Method::POST, &path, &query, Some(batch))
            .await?;
        crate::rest_response::parse_query_batch(result)
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
        wait: bool,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        let path = format!("/collections/{collection}/points/batch");
        let wait = if wait { "true" } else { "false" };
        let result: Value = self
            .execute_typed(Method::POST, &path, &[("wait", wait)], Some(batch))
            .await?;
        crate::rest_response::parse_update_batch(result)
    }

    async fn change_aliases(&self, actions: &[crate::client::AliasAction]) -> Result<(), QqlError> {
        let body = serde_json::json!({
            "actions": actions.iter().map(|a| match a {
                crate::client::AliasAction::Delete { alias } => {
                    serde_json::json!({ "delete_alias": { "alias_name": alias } })
                }
                crate::client::AliasAction::Create { collection, alias } => {
                    serde_json::json!({
                        "create_alias": {
                            "collection_name": collection,
                            "alias_name": alias,
                        }
                    })
                }
            }).collect::<Vec<_>>(),
        });
        self.execute_typed::<_, serde::de::IgnoredAny>(
            Method::POST,
            "/collections/aliases",
            &[],
            Some(&body),
        )
        .await?;
        Ok(())
    }
}

impl RestQdrant {
    /// CREATE COLLECTION as the plan-owned REST sequence
    /// (`PUT` body, conditional `PATCH`, per-key shard `PUT`s): the step plan
    /// lives in `qql_plan::ddl_rest`, the transport only sends the steps
    /// (parity with the gRPC multi-step create).
    async fn create_collection_planned(
        &self,
        collection: &str,
        req: &qql_plan::types::CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        let steps =
            qql_plan::ddl::create_collection_rest_steps(collection, req).map_err(|error| {
                QqlError::execution(
                    "QQL-PLAN-SERIALIZE",
                    format!("plan IR REST request body serialization failed: {error}"),
                    None,
                )
            })?;
        for step in steps {
            let method = match step.method {
                PlanMethod::Get => Method::GET,
                PlanMethod::Post => Method::POST,
                PlanMethod::Put => Method::PUT,
                PlanMethod::Patch => Method::PATCH,
                PlanMethod::Delete => Method::DELETE,
            };
            self.execute_typed::<_, serde::de::IgnoredAny>(
                method,
                &step.path,
                &[],
                Some(&step.body),
            )
            .await?;
        }
        Ok(())
    }

    /// Low-level HTTP dispatch from a pre-built Route.
    async fn execute_http(&self, route: qql_plan::routing::Route) -> Result<Value, QqlError> {
        let method = match route.method {
            PlanMethod::Get => Method::GET,
            PlanMethod::Post => Method::POST,
            PlanMethod::Put => Method::PUT,
            PlanMethod::Patch => Method::PATCH,
            PlanMethod::Delete => Method::DELETE,
        };

        let url = format!("{}{}", self.base_url, route.path);
        let request_id = next_request_id();
        let mut builder = match method {
            Method::GET => self.client.get(&url),
            Method::POST => self.client.post(&url),
            Method::PUT => self.client.put(&url),
            Method::PATCH => self.client.patch(&url),
            Method::DELETE => self.client.delete(&url),
            _ => self.client.request(method, &url),
        };
        if !route.query.is_empty() {
            builder = builder.query(&route.query);
        }
        builder = self.apply_headers(builder, &request_id);
        if let Some(ref body) = route.body {
            builder = builder.json(body);
        }
        let resp = builder.send().await.map_err(|e| {
            QqlError::transport(
                "QQL-TRANSPORT-REQUEST",
                format!("REST request failed: {e} (request id: {request_id})"),
                None,
            )
            .with_url(url.clone())
            .with_field("request_id", request_id.clone())
        })?;
        let status = resp.status();
        let server_request_id = resp
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| request_id.clone());
        let text = resp.text().await.map_err(|e| {
            QqlError::transport(
                "QQL-TRANSPORT-REQUEST",
                format!("REST body read failed: {e} (request id: {request_id})"),
                None,
            )
        })?;
        if !status.is_success() {
            let code = classify_backend_error_code(status.as_u16(), &text);
            return Err(QqlError::backend(
                code,
                format!("REST {status}: {text} (request id: {server_request_id})"),
                None,
            )
            .with_status(status.as_u16())
            .with_url(url)
            .with_field("request_id", server_request_id));
        }
        let value: Value = serde_json::from_str(&text).map_err(|e| {
            QqlError::backend(
                "QQL-BACKEND-JSON",
                format!(
                    "invalid JSON response: {e}; body={text} (request id: {server_request_id})"
                ),
                None,
            )
            .with_field("request_id", server_request_id)
        })?;
        validate_success_envelope(&value, &route.path)?;
        Ok(value)
    }
}

fn validate_success_envelope(value: &Value, operation: &str) -> Result<(), QqlError> {
    let object = value.as_object().ok_or_else(|| {
        QqlError::backend(
            "QQL-BACKEND-ENVELOPE",
            format!("{operation} returned a non-object JSON response"),
            None,
        )
    })?;

    if !object.contains_key("result") {
        return Err(QqlError::backend(
            "QQL-BACKEND-ENVELOPE",
            format!("{operation} response is missing the result field"),
            None,
        ));
    }

    if object.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(QqlError::backend(
            "QQL-BACKEND-ENVELOPE",
            format!("{operation} response is missing status=ok"),
            None,
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn accepts_qdrant_success_envelope() {
        let value = serde_json::json!({
            "result": [],
            "status": "ok",
            "time": 0.001,
        });
        assert!(validate_success_envelope(&value, "test").is_ok());
    }

    #[test]
    fn rejects_missing_result() {
        let value = serde_json::json!({ "status": "ok" });
        let error = validate_success_envelope(&value, "test").unwrap_err();
        assert_eq!(error.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn rejects_non_ok_status() {
        let value = serde_json::json!({ "result": [], "status": "error" });
        let error = validate_success_envelope(&value, "test").unwrap_err();
        assert_eq!(error.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn collection_missing_predicate_matches_404_shapes() {
        // Synthetic 404-ish failures (status echoed in the message, or a
        // "Not found" body) map to `Ok(false)` in `collection_exists`.
        assert!(is_collection_missing_message(
            "Qdrant returned 404 Not Found: {\"status\":{\"error\":\"Not found: Collection docs not found\"}}"
        ));
        assert!(is_collection_missing_message("collection Not found"));
        // Anything else propagates as an error.
        assert!(!is_collection_missing_message(
            "HTTP request failed: connection refused"
        ));
        // Lowercase body text (e.g. "index not found" inside a 200 envelope)
        // must not read as collection-missing (RT-09 asymmetry).
        assert!(!is_collection_missing_message("index not found"));
    }

    #[test]
    fn debug_redacts_api_key() {
        let rest = RestQdrant::with_timeout(
            "http://localhost:6333",
            Some("super-secret".into()),
            Duration::from_secs(1),
        )
        .expect("client");
        let debug = format!("{rest:?}");
        assert!(!debug.contains("super-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }
}
