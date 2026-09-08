//! Browser `Client`: gloo-net transport, embedder configuration, topology.
//!
//! Execution entry points live in [`super::execute`]; the `qql-embed`
//! adapter lives in [`super::embed`].

#[cfg(all(feature = "client", target_arch = "wasm32"))]
use gloo_net::http::Request;
use serde_json::json;
use wasm_bindgen::prelude::*;

enum EmbedMode {
    None,
    /// JS function: `async (texts: string[]) => number[][]` (already batched).
    Js(js_sys::Function),
    /// OpenAI-compatible HTTP: POST `{"model", "input": string[]}`.
    /// User must supply the full endpoint (OpenAI, Ollama `/v1/embeddings`, etc.).
    Http,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[wasm_bindgen]
pub struct Client {
    url: String,
    api_key: Option<String>,
    /// Qdrant 1.19 read affinity (`X-Qdrant-Route-Affinity` header). Pins
    /// reads to a stable replica; `None` = unset.
    route_affinity: Option<String>,

    embed_mode: EmbedMode,
    embed_endpoint: String,
    embed_api_key: Option<String>,
    embed_model: String,
    embed_dim: u32,
}

#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    pub fn new(url: Option<String>, api_key: Option<String>) -> Client {
        Client {
            url: url.unwrap_or_else(|| "http://localhost:6333".to_string()),
            api_key,
            route_affinity: None,
            embed_mode: EmbedMode::None,
            embed_endpoint: String::new(),
            embed_api_key: None,
            embed_model: String::new(),
            embed_dim: 0,
        }
    }

    /// Set Qdrant 1.19 read affinity. Pins reads to a stable replica via the
    /// `X-Qdrant-Route-Affinity` header. Pass `null`/`""` to clear.
    #[wasm_bindgen(js_name = setRouteAffinity)]
    pub fn set_route_affinity(&mut self, affinity: Option<String>) {
        self.route_affinity = affinity.filter(|s| !s.is_empty());
    }

    /// Current read-affinity key, or `null` when unset.
    #[wasm_bindgen(getter, js_name = routeAffinity)]
    pub fn route_affinity(&self) -> Option<String> {
        self.route_affinity.clone()
    }

    // ── Embedder configuration ──────────────────────────────────

    /// Set a JS embedder: `async (texts: string[]) => number[][]`.
    /// Called with the full batch — do not loop one-by-one inside the callback
    /// if your model supports batching (Transformers.js pipeline, etc.).
    #[wasm_bindgen(js_name = setEmbedder)]
    pub fn set_embedder(
        &mut self,
        #[wasm_bindgen(
            unchecked_param_type = "(texts: string[]) => Promise<number[][]> | number[][]"
        )]
        fn_: js_sys::Function,
    ) {
        self.embed_mode = EmbedMode::Js(fn_);
    }

    /// OpenAI-compatible HTTP embedder. **No default URL** — pass the full
    /// embeddings endpoint you intend to use, e.g.:
    /// - `https://api.openai.com/v1/embeddings`
    /// - `http://localhost:11434/v1/embeddings` (Ollama)
    /// - any provider that accepts `{"model","input":[...]}` and returns
    ///   `{"data":[{"embedding":[...],"index":0},...]}`.
    ///
    /// Always sends the whole text batch in one request (`input` as array).
    #[wasm_bindgen(js_name = setHttpEmbedder)]
    pub fn set_http_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        if endpoint.trim().is_empty() {
            return Err(JsValue::from_str(
                "setHttpEmbedder: endpoint is required (no default URL)",
            ));
        }
        if model.trim().is_empty() {
            return Err(JsValue::from_str("setHttpEmbedder: model is required"));
        }
        if dimension == 0 {
            return Err(JsValue::from_str(
                "setHttpEmbedder: dimension must be positive",
            ));
        }
        self.embed_mode = EmbedMode::Http;
        self.embed_endpoint = endpoint;
        self.embed_api_key = api_key;
        self.embed_model = model;
        self.embed_dim = dimension;
        Ok(())
    }

    /// Alias for [`set_http_embedder`] — same OpenAI-compatible protocol.
    #[wasm_bindgen(js_name = setRemoteEmbedder)]
    pub fn set_remote_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        self.set_http_embedder(endpoint, model, dimension, api_key)
    }

    /// Check whether any embedder is configured.
    #[wasm_bindgen(js_name = hasEmbedder)]
    pub fn has_embedder(&self) -> bool {
        !matches!(self.embed_mode, EmbedMode::None)
    }

    pub(crate) fn request(&self, method: &str, path: &str) -> gloo_net::http::RequestBuilder {
        let mut rb = match method {
            "GET" => Request::get(&format!("{}{}", self.url, path)),
            "POST" => Request::post(&format!("{}{}", self.url, path)),
            "PUT" => Request::put(&format!("{}{}", self.url, path)),
            "PATCH" => Request::patch(&format!("{}{}", self.url, path)),
            "DELETE" => Request::delete(&format!("{}{}", self.url, path)),
            _ => Request::get(&format!("{}{}", self.url, path)),
        };
        if let Some(ref key) = self.api_key {
            rb = rb.header("api-key", key);
        }
        if let Some(ref affinity) = self.route_affinity {
            rb = rb.header("X-Qdrant-Route-Affinity", affinity);
        }
        rb = rb.header("Content-Type", "application/json");
        rb
    }

    /// Embed a batch of texts. Returns vectors in the same order.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, JsValue> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        match &self.embed_mode {
            EmbedMode::Js(fn_) => {
                let array = js_sys::Array::new();
                for t in &texts {
                    array.push(&JsValue::from_str(t));
                }
                let returned = fn_
                    .call1(&JsValue::NULL, &array)
                    .map_err(|e| JsValue::from_str(&format!("embedder call failed: {:?}", e)))?;

                let result = if returned.is_instance_of::<js_sys::Promise>() {
                    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(returned))
                        .await
                        .map_err(|e| JsValue::from_str(&format!("embedder rejected: {:?}", e)))?
                } else {
                    returned
                };

                let rows: Vec<Vec<f32>> = serde_wasm_bindgen::from_value(result).map_err(|e| {
                    JsValue::from_str(&format!("embedder returned invalid vectors: {}", e))
                })?;

                if rows.len() != texts.len() {
                    return Err(JsValue::from_str(&format!(
                        "embedder returned {} vectors, expected {}",
                        rows.len(),
                        texts.len()
                    )));
                }
                Ok(rows)
            }

            EmbedMode::Http => {
                // Single HTTP request: input = full array (OpenAI/Ollama/Cohere compat).
                let body = json!({ "model": self.embed_model, "input": texts });
                let resp = self.post_with_auth(&self.embed_endpoint, &body).await?;
                Self::parse_openai_batch_response(&resp, texts.len(), self.embed_dim)
            }

            EmbedMode::None => Ok(Vec::new()),
        }
    }

    /// POST JSON with Bearer auth to embedding endpoint.
    #[cfg(target_arch = "wasm32")]
    async fn post_with_auth(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, JsValue> {
        let body_str =
            serde_json::to_string(body).map_err(|e| JsValue::from_str(&e.to_string()))?;

        let mut rb = Request::post(url).header("Content-Type", "application/json");
        if let Some(ref key) = self.embed_api_key {
            rb = rb.header("Authorization", &format!("Bearer {}", key));
        }

        let resp = rb
            .body(body_str)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .send()
            .await
            .map_err(|e| JsValue::from_str(&format!("embedding API error: {}", e)))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        if status >= 400 {
            return Err(JsValue::from_str(&format!(
                "embedding API returned {}: {}",
                status, text
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| JsValue::from_str(&format!("invalid embedding API response: {}", e)))
    }

    /// Parse OpenAI-compatible batch response:
    /// `{"data":[{"embedding":[...],"index":0}, ...]}` — reorders by `index` when present.
    #[cfg(target_arch = "wasm32")]
    fn parse_openai_batch_response(
        resp: &serde_json::Value,
        expected: usize,
        expected_dim: u32,
    ) -> Result<Vec<Vec<f32>>, JsValue> {
        let data = resp["data"]
            .as_array()
            .ok_or_else(|| JsValue::from_str("embedding response missing 'data' array"))?;

        let mut slots: Vec<Option<Vec<f32>>> = vec![None; expected];
        for (fallback_i, item) in data.iter().enumerate() {
            let emb = item["embedding"]
                .as_array()
                .ok_or_else(|| JsValue::from_str("item missing 'embedding' array"))?;
            if expected_dim > 0 && emb.len() != expected_dim as usize {
                return Err(JsValue::from_str(&format!(
                    "embedding dimension mismatch: got {}, expected {}",
                    emb.len(),
                    expected_dim
                )));
            }
            let vec: Vec<f32> = emb
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            let idx = item["index"].as_u64().unwrap_or(fallback_i as u64) as usize;
            if idx >= expected {
                return Err(JsValue::from_str(&format!(
                    "embedding index {idx} out of range (batch size {expected})"
                )));
            }
            if slots[idx].is_some() {
                return Err(JsValue::from_str(&format!(
                    "duplicate embedding index {idx}"
                )));
            }
            slots[idx] = Some(vec);
        }

        slots
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                v.ok_or_else(|| JsValue::from_str(&format!("missing embedding at index {i}")))
            })
            .collect()
    }

    /// Shared AST resolve via `qql-embed` (batched dense + local sparse).
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn resolve_stmt_embeddings(
        &self,
        stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        if !self.has_embedder() {
            return Ok(());
        }
        qql_embed::resolve_embeddings(stmt, self)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn resolve_stmt_embeddings(
        &self,
        _stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        Ok(())
    }
    /// Fetch collection topology and resolve `USING` vector kinds.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn resolve_stmt_vector_kinds(
        &self,
        stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        let qql_core::ast::Stmt::Query(query) = stmt else {
            return Ok(());
        };
        let qql_core::ast::QueryCollection::Explicit(collection) = &query.collection else {
            return Ok(());
        };
        if !qql_embed::query_needs_kind_resolution(query) {
            return Ok(());
        }
        let collection = collection.clone();
        let topology = self.fetch_vector_topology(&collection).await?;
        qql_embed::resolve_query_vector_kinds(&collection, query, &topology)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn resolve_stmt_vector_kinds(
        &self,
        _stmt: &mut qql_core::ast::Stmt,
    ) -> Result<(), JsValue> {
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn fetch_vector_topology(
        &self,
        collection: &str,
    ) -> Result<qql_embed::TopologyNames, JsValue> {
        use super::response::vector_names_from_collection_result;
        let path = format!("/collections/{collection}");
        let body = self.send_json("GET", &path, None).await?;
        let result = body
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(vector_names_from_collection_result(&result))
    }
    pub(crate) async fn send_json(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsValue> {
        let body_str = body
            .as_ref()
            .map(|b| serde_json::to_string(b).map_err(|e| JsValue::from_str(&e.to_string())))
            .transpose()?;

        let rb = self.request(method, path);
        let resp = if let Some(s) = body_str {
            rb.body(s)
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .send()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        } else {
            rb.send()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
        };

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        if status >= 400 {
            return Err(JsValue::from_str(&format!(
                "Qdrant returned {}: {}",
                status, text
            )));
        }

        serde_json::from_str(&text).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
