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
    multi_endpoint: Option<String>,
    multi_api_key: Option<String>,
    multi_model: Option<String>,
    multi_dim: u32,
    image_endpoint: Option<String>,
    image_api_key: Option<String>,
    image_model: Option<String>,
    image_dim: u32,
    rerank_endpoint: Option<String>,
    rerank_api_key: Option<String>,
    rerank_model: Option<String>,
    /// Client-side BM25 document parameters for the built-in local sparse
    /// encoder (used for `TEXT` upserts / sparse `TEXT` inputs; write-path
    /// only). Defaults to the Qdrant `qdrant/bm25` values.
    pub(crate) bm25: qql_embed::Bm25Params,
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
            multi_endpoint: None,
            multi_api_key: None,
            multi_model: None,
            multi_dim: 0,
            image_endpoint: None,
            image_api_key: None,
            image_model: None,
            image_dim: 0,
            rerank_endpoint: None,
            rerank_api_key: None,
            rerank_model: None,
            bm25: qql_embed::Bm25Params::default(),
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

    /// Set client-side BM25 document parameters for the built-in local sparse
    /// encoder (`k1`, `b`, `avg_len`). **Write-path only**: shapes how
    /// documents upserted after the call are encoded; query weights stay unit
    /// and server-side inference is untouched. Invalid values throw
    /// (`QQL-VALIDATION-CONFIG`): `k1 > 0`, `b` in `[0, 1]`, `avg_len > 0`,
    /// all finite.
    #[wasm_bindgen(js_name = setBm25Params)]
    pub fn set_bm25_params(&mut self, k1: f64, b: f64, avg_len: f64) -> Result<(), JsValue> {
        let params = qql_embed::Bm25Params::new(k1, b, avg_len)
            .map_err(|e| JsValue::from_str(&format!("{}: {}", e.code, e.message)))?;
        self.bm25 = params;
        Ok(())
    }

    /// Current BM25 document parameters (used by the local sparse encoder).
    pub(crate) fn bm25_params(&self) -> &qql_embed::Bm25Params {
        &self.bm25
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

    /// OpenAI-compatible multi/ColBERT endpoint (nested `[[...]]` bags).
    /// Browser calls need a CORS-enabled endpoint.
    #[wasm_bindgen(js_name = setHttpMultiEmbedder)]
    pub fn set_http_multi_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        if endpoint.trim().is_empty() {
            return Err(JsValue::from_str(
                "setHttpMultiEmbedder: endpoint is required (no default URL)",
            ));
        }
        if model.trim().is_empty() {
            return Err(JsValue::from_str("setHttpMultiEmbedder: model is required"));
        }
        if dimension == 0 {
            return Err(JsValue::from_str(
                "setHttpMultiEmbedder: dimension must be positive",
            ));
        }
        self.multi_endpoint = Some(endpoint);
        self.multi_api_key = api_key;
        self.multi_model = Some(model);
        self.multi_dim = dimension;
        Ok(())
    }

    /// OpenAI-compatible image/CLIP vision endpoint (dense vectors).
    /// Browser calls need a CORS-enabled endpoint.
    #[wasm_bindgen(js_name = setHttpImageEmbedder)]
    pub fn set_http_image_embedder(
        &mut self,
        endpoint: String,
        model: String,
        dimension: u32,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        if endpoint.trim().is_empty() {
            return Err(JsValue::from_str(
                "setHttpImageEmbedder: endpoint is required (no default URL)",
            ));
        }
        if model.trim().is_empty() {
            return Err(JsValue::from_str("setHttpImageEmbedder: model is required"));
        }
        if dimension == 0 {
            return Err(JsValue::from_str(
                "setHttpImageEmbedder: dimension must be positive",
            ));
        }
        self.image_endpoint = Some(endpoint);
        self.image_api_key = api_key;
        self.image_model = Some(model);
        self.image_dim = dimension;
        Ok(())
    }

    /// Cohere-compatible cross-encoder rerank endpoint.
    /// Browser calls need a CORS-enabled endpoint.
    #[wasm_bindgen(js_name = setHttpReranker)]
    pub fn set_http_reranker(
        &mut self,
        endpoint: String,
        model: String,
        api_key: Option<String>,
    ) -> Result<(), JsValue> {
        if endpoint.trim().is_empty() {
            return Err(JsValue::from_str(
                "setHttpReranker: endpoint is required (no default URL)",
            ));
        }
        if model.trim().is_empty() {
            return Err(JsValue::from_str("setHttpReranker: model is required"));
        }
        self.rerank_endpoint = Some(endpoint);
        self.rerank_api_key = api_key;
        self.rerank_model = Some(model);
        Ok(())
    }

    /// Check whether any embedder is configured.
    #[wasm_bindgen(js_name = hasEmbedder)]
    pub fn has_embedder(&self) -> bool {
        !matches!(self.embed_mode, EmbedMode::None)
            || self.multi_enabled()
            || self.image_enabled()
            || self.rerank_enabled()
    }

    pub(crate) fn multi_enabled(&self) -> bool {
        self.multi_endpoint.is_some() || self.multi_model.is_some() || self.multi_dim > 0
    }

    pub(crate) fn image_enabled(&self) -> bool {
        self.image_endpoint.is_some() || self.image_model.is_some() || self.image_dim > 0
    }

    pub(crate) fn rerank_enabled(&self) -> bool {
        self.rerank_endpoint.is_some() || self.rerank_model.is_some()
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
        self.post_with_key(url, body, self.embed_api_key.as_deref())
            .await
    }

    #[cfg(target_arch = "wasm32")]
    async fn post_with_key(
        &self,
        url: &str,
        body: &serde_json::Value,
        api_key: Option<&str>,
    ) -> Result<serde_json::Value, JsValue> {
        let body_str =
            serde_json::to_string(body).map_err(|e| JsValue::from_str(&e.to_string()))?;

        let mut rb = Request::post(url).header("Content-Type", "application/json");
        if let Some(key) = api_key.filter(|k| !k.is_empty()) {
            rb = rb.header("Authorization", &format!("Bearer {key}"));
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

    #[cfg(target_arch = "wasm32")]
    fn multi_url(&self) -> &str {
        self.multi_endpoint
            .as_deref()
            .unwrap_or(self.embed_endpoint.as_str())
    }

    #[cfg(target_arch = "wasm32")]
    fn multi_key(&self) -> Option<&str> {
        self.multi_api_key
            .as_deref()
            .or(self.embed_api_key.as_deref())
    }

    #[cfg(target_arch = "wasm32")]
    fn resolve_multi_model<'a>(&'a self, model: &'a str) -> &'a str {
        if !model.is_empty() && model != "default" {
            model
        } else {
            self.multi_model
                .as_deref()
                .unwrap_or(self.embed_model.as_str())
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn image_url(&self) -> &str {
        self.image_endpoint
            .as_deref()
            .unwrap_or(self.embed_endpoint.as_str())
    }

    #[cfg(target_arch = "wasm32")]
    fn image_key(&self) -> Option<&str> {
        self.image_api_key
            .as_deref()
            .or(self.embed_api_key.as_deref())
    }

    #[cfg(target_arch = "wasm32")]
    fn resolve_image_model<'a>(&'a self, model: &'a str) -> &'a str {
        if !model.is_empty() && model != "default" {
            model
        } else {
            self.image_model
                .as_deref()
                .unwrap_or(self.embed_model.as_str())
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn image_dim(&self) -> u32 {
        if self.image_dim > 0 {
            self.image_dim
        } else {
            self.embed_dim
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn rerank_key(&self) -> Option<&str> {
        self.rerank_api_key
            .as_deref()
            .or(self.embed_api_key.as_deref())
    }

    /// Embed multi/ColBERT texts via HTTP (nested `[[...]]` bags, flat rejected).
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn embed_multi_texts(
        &self,
        texts: Vec<String>,
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, JsValue> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if !self.multi_enabled() {
            return Err(JsValue::from_str(
                "multi-vector embedding is not available (no model specified). Configure setHttpMultiEmbedder, pass precomputed VECTOR [[...], ...], or use UPSERT with explicit multivector bags.",
            ));
        }
        let model_name = self.resolve_multi_model(model);
        let body = json!({ "model": model_name, "input": texts });
        let resp = self
            .post_with_key(self.multi_url(), &body, self.multi_key())
            .await?;
        Self::parse_multi_batch_response(&resp, texts.len(), self.multi_dim)
    }

    /// Embed image paths/URLs via HTTP (dense vectors).
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn embed_image_sources(
        &self,
        sources: Vec<String>,
        model: &str,
    ) -> Result<Vec<Vec<f32>>, JsValue> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        if !self.image_enabled() {
            return Err(JsValue::from_str(
                "image embedding is not available (no model specified). Configure setHttpImageEmbedder, pass a precomputed VECTOR [...], or use UPSERT USING IMAGE ON FIELD <path_field>.",
            ));
        }
        let model_name = self.resolve_image_model(model);
        let body = json!({ "model": model_name, "input": sources });
        let resp = self
            .post_with_key(self.image_url(), &body, self.image_key())
            .await?;
        Self::parse_openai_batch_response(&resp, sources.len(), self.image_dim())
    }

    /// Score (query, documents) pairs via Cohere-compatible rerank endpoint.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn rerank_pair_scores(
        &self,
        query: &str,
        documents: &[String],
        model: &str,
    ) -> Result<Vec<f32>, JsValue> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let Some(endpoint) = self.rerank_endpoint.as_deref() else {
            return Err(JsValue::from_str(
                "cross-encoder pair scoring is not available (no model specified). Configure setHttpReranker.",
            ));
        };
        let model_name = if !model.is_empty() && model != "default" {
            model.to_string()
        } else {
            self.rerank_model
                .clone()
                .unwrap_or_else(|| "rerank".to_string())
        };
        let body = json!({ "model": model_name, "query": query, "documents": documents });
        let resp = self
            .post_with_key(endpoint, &body, self.rerank_key())
            .await?;
        let results = resp
            .get("results")
            .or_else(|| resp.get("data"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| JsValue::from_str("rerank response missing results array"))?;
        let mut scores = vec![0.0f32; documents.len()];
        let mut seen = vec![false; documents.len()];
        for item in results {
            let idx = item
                .get("index")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsValue::from_str("rerank result missing index"))?
                as usize;
            if idx >= documents.len() {
                return Err(JsValue::from_str(&format!(
                    "rerank result index {idx} out of range"
                )));
            }
            let score = item
                .get("relevance_score")
                .or_else(|| item.get("score"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            scores[idx] = score;
            seen[idx] = true;
        }
        if seen.iter().any(|s| !*s) {
            return Err(JsValue::from_str(
                "rerank response did not cover all documents",
            ));
        }
        Ok(scores)
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

    /// Parse multi/ColBERT batch response: `embedding` must be nested
    /// `[[f32]]`; flat dense arrays are rejected like the Rust core.
    #[cfg(target_arch = "wasm32")]
    fn parse_multi_batch_response(
        resp: &serde_json::Value,
        expected: usize,
        expected_dim: u32,
    ) -> Result<Vec<Vec<Vec<f32>>>, JsValue> {
        let data = resp["data"]
            .as_array()
            .ok_or_else(|| JsValue::from_str("embedding response missing 'data' array"))?;
        let mut slots: Vec<Option<Vec<Vec<f32>>>> = vec![None; expected];
        for (fallback_i, item) in data.iter().enumerate() {
            let emb = item["embedding"]
                .as_array()
                .ok_or_else(|| JsValue::from_str("item missing 'embedding' array"))?;
            if emb.is_empty() {
                return Err(JsValue::from_str("multi embedding returned empty bag"));
            }
            // Flat dense `[...f32]` rejected: first element must be an array.
            if emb.first().is_some_and(|v| !v.is_array()) {
                return Err(JsValue::from_str(&format!(
                    "multi embedding endpoint returned a flat dense vector (len={}) for index {}; expected nested array [[f32,…],…] (token-level multivector)",
                    emb.len(),
                    fallback_i
                )));
            }
            let mut rows = Vec::with_capacity(emb.len());
            for row in emb {
                let arr = row.as_array().ok_or_else(|| {
                    JsValue::from_str("multi embedding row must be an array of numbers")
                })?;
                if expected_dim > 0 && arr.len() != expected_dim as usize {
                    return Err(JsValue::from_str(&format!(
                        "multi embedding dimension mismatch: got {}, expected {}",
                        arr.len(),
                        expected_dim
                    )));
                }
                rows.push(
                    arr.iter()
                        .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                        .collect::<Vec<f32>>(),
                );
            }
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
            slots[idx] = Some(rows);
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
        use super::schema::vector_names_from_collection_result;
        let path = format!("/collections/{collection}");
        let body = self.send_json("GET", &path, None).await?;
        let result = body
            .get("result")
            .filter(|value| value.is_object())
            .ok_or_else(|| {
                JsValue::from_str(
                    &serde_json::to_string(&qql_core::error::QqlError::backend(
                        "QQL-BACKEND-ENVELOPE",
                        "get collection response is missing a result object",
                        None,
                    ))
                    .unwrap_or_else(|_| {
                        "get collection response is missing a result object".into()
                    }),
                )
            })?;
        Ok(vector_names_from_collection_result(result))
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
