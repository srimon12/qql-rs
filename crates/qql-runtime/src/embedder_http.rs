//! Reqwest `HttpEmbedder` client for OpenAI-compatible embedding endpoints.
//!
//! Split from `embedder.rs` (size hygiene): the HTTP wire types, the
//! `HttpEmbedder` struct with its constructors, and the `Embedder`
//! implementation. `HttpEmbedderOptions` and the shared `qql-embed`
//! re-exports stay in `embedder`. Behavior is unchanged.

#[cfg(feature = "rest")]
use async_trait::async_trait;
#[cfg(feature = "rest")]
use reqwest::Client;
#[cfg(feature = "rest")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rest")]
use qql_core::error::QqlError;
#[cfg(feature = "rest")]
use qql_embed::bm25_text::{Bm25Pipeline, Bm25TextConfig};
#[cfg(feature = "rest")]
use qql_embed::embedder::Embedder;
use qql_embed::sparse::{Bm25Params, SparseVector};

#[cfg(feature = "rest")]
use super::embedder::HttpEmbedderOptions;

#[cfg(feature = "rest")]
#[derive(Debug, Clone, Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[cfg(feature = "rest")]
#[derive(Debug, Clone, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

/// OpenAI-compatible embedding payload: dense `[f32]` or multi `[[f32],…]`.
#[cfg(feature = "rest")]
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EmbeddingPayload {
    Dense(Vec<f32>),
    Multi(Vec<Vec<f32>>),
}

#[cfg(feature = "rest")]
#[derive(Debug, Clone, Deserialize)]
struct EmbedData {
    index: usize,
    embedding: EmbeddingPayload,
}
/// OpenAI-compatible HTTP embedder (`POST {"model","input":[...]}`).
///
/// Endpoint is **required** — no default URL. Works with OpenAI, Ollama
/// `/v1/embeddings`, Cohere compatibility API, etc. Always batches in one request.
///
/// Multivector: set `multi_*` options (or share the dense endpoint with a model that
/// returns nested `embedding: [[…],…]` arrays). Flat dense arrays on multi requests
/// are rejected.
#[cfg(feature = "rest")]
pub struct HttpEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dimension: usize,
    multi_endpoint: Option<String>,
    multi_api_key: Option<String>,
    multi_model: Option<String>,
    multi_dimension: usize,
    image_endpoint: Option<String>,
    image_api_key: Option<String>,
    image_model: Option<String>,
    image_dimension: usize,
    rerank_endpoint: Option<String>,
    rerank_api_key: Option<String>,
    rerank_model: Option<String>,
    bm25_text: Bm25TextConfig,
    bm25_pipeline: Bm25Pipeline,
    client: Client,
}

#[cfg(feature = "rest")]
impl HttpEmbedder {
    /// Build an embedder with only dense settings; see `try_with_options` for
    /// multi, image, and rerank configuration.
    pub fn new(
        endpoint: String,
        api_key: String,
        model: String,
        dimension: usize,
    ) -> Result<Self, QqlError> {
        Self::try_with_options(HttpEmbedderOptions {
            endpoint,
            api_key,
            model,
            dimension,
            ..Default::default()
        })
    }

    /// Validate options and build the embedder; endpoint, model, and positive
    /// dimension are required.
    pub fn try_with_options(opts: HttpEmbedderOptions) -> Result<Self, QqlError> {
        if opts.endpoint.trim().is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding endpoint is required",
                None,
            ));
        }
        if opts.model.trim().is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding model is required",
                None,
            ));
        }
        if opts.dimension == 0 {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding dimension must be positive",
                None,
            ));
        }

        let bm25_text = opts.bm25_text_config()?;
        let bm25_pipeline = bm25_text.pipeline();

        let client = Client::builder().build().map_err(|e| {
            QqlError::execution(
                "QQL-EMBEDDING",
                format!("failed to create HTTP client: {}", e),
                None,
            )
        })?;

        Ok(HttpEmbedder {
            endpoint: opts.endpoint,
            api_key: opts.api_key,
            model: opts.model,
            dimension: opts.dimension,
            multi_endpoint: opts.multi_endpoint.filter(|s| !s.trim().is_empty()),
            multi_api_key: opts.multi_api_key,
            multi_model: opts.multi_model.filter(|s| !s.trim().is_empty()),
            multi_dimension: opts.multi_dimension,
            image_endpoint: opts.image_endpoint.filter(|s| !s.trim().is_empty()),
            image_api_key: opts.image_api_key,
            image_model: opts.image_model.filter(|s| !s.trim().is_empty()),
            image_dimension: opts.image_dimension,
            rerank_endpoint: opts.rerank_endpoint.filter(|s| !s.trim().is_empty()),
            rerank_api_key: opts.rerank_api_key,
            rerank_model: opts.rerank_model.filter(|s| !s.trim().is_empty()),
            bm25_text,
            bm25_pipeline,
            client,
        })
    }

    /// Attach multi/ColBERT settings after construction.
    pub fn with_multi(
        mut self,
        endpoint: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
        dimension: usize,
    ) -> Self {
        self.multi_endpoint = endpoint.filter(|s| !s.trim().is_empty());
        self.multi_api_key = api_key;
        self.multi_model = model.filter(|s| !s.trim().is_empty());
        self.multi_dimension = dimension;
        self
    }

    /// Attach image/CLIP vision settings after construction.
    pub fn with_image(
        mut self,
        endpoint: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
        dimension: usize,
    ) -> Self {
        self.image_endpoint = endpoint.filter(|s| !s.trim().is_empty());
        self.image_api_key = api_key;
        self.image_model = model.filter(|s| !s.trim().is_empty());
        self.image_dimension = dimension;
        self
    }

    /// Whether multi/ColBERT embedding is configured on this embedder.
    pub fn multi_enabled(&self) -> bool {
        self.multi_model.is_some() || self.multi_endpoint.is_some() || self.multi_dimension > 0
    }

    /// Whether image/CLIP vision embedding is configured on this embedder.
    pub fn image_enabled(&self) -> bool {
        self.image_model.is_some() || self.image_endpoint.is_some() || self.image_dimension > 0
    }

    /// Whether cross-encoder pair rerank is configured on this embedder.
    pub fn rerank_enabled(&self) -> bool {
        self.rerank_endpoint.is_some() || self.rerank_model.is_some()
    }

    /// Embed one sample input and return the observed vector dimension.
    pub async fn probe_dimension(&self, input: &str) -> Result<usize, QqlError> {
        let input = [input.to_string()];
        let body = EmbedRequest {
            model: &self.model,
            input: &input,
        };

        let resp = self
            .do_request(&self.endpoint, &self.api_key, &body)
            .await?;

        if resp.data.is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding response contained no vectors",
                None,
            ));
        }

        match &resp.data[0].embedding {
            EmbeddingPayload::Dense(v) => Ok(v.len()),
            EmbeddingPayload::Multi(rows) => rows.first().map(Vec::len).ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    "embedding response multivector was empty",
                    None,
                )
            }),
        }
    }

    async fn do_request(
        &self,
        endpoint: &str,
        api_key: &str,
        body: &EmbedRequest<'_>,
    ) -> Result<EmbedResponse, QqlError> {
        let mut req = self.client.post(endpoint).json(body);

        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }

        let resp = req.send().await.map_err(|e| {
            if e.is_connect() {
                QqlError::execution(
                    "QQL-EMBEDDING-UNAVAILABLE",
                    format!(
                        "failed to connect to embedding endpoint at '{endpoint}': {e}\n\
                        → Ensure your embedding server (e.g. Ollama, vLLM) is running at '{endpoint}'\n\
                        → Or pass precomputed vectors via --params-file <file.json>\n\
                        → Or install with local zero-server ONNX embeddings: cargo install qql-cli --locked --features fastembed"
                    ),
                    None,
                )
            } else {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("failed to call embedding endpoint: {}", e),
                    None,
                )
            }
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                format!("embedding endpoint returned {}: {}", status, text),
                None,
            ));
        }

        let decoded: EmbedResponse = resp.json().await.map_err(|e| {
            QqlError::execution(
                "QQL-EMBEDDING",
                format!("failed to decode embedding response: {}", e),
                None,
            )
        })?;

        Ok(decoded)
    }

    fn resolve_dense_model<'a>(&'a self, model: &'a str) -> &'a str {
        if !model.is_empty() && model != "default" {
            model
        } else {
            self.model.as_str()
        }
    }

    fn resolve_multi_model<'a>(&'a self, model: &'a str) -> &'a str {
        if !model.is_empty() && model != "default" {
            model
        } else {
            self.multi_model.as_deref().unwrap_or(self.model.as_str())
        }
    }

    fn multi_url(&self) -> &str {
        self.multi_endpoint
            .as_deref()
            .unwrap_or(self.endpoint.as_str())
    }

    fn multi_key(&self) -> &str {
        self.multi_api_key
            .as_deref()
            .unwrap_or(self.api_key.as_str())
    }

    /// Scatter an indexed embedding response into input order.
    ///
    /// Shared skeleton behind the dense / multi / image batch validators:
    /// cardinality check, index-range + duplicate guards, per-item payload
    /// extraction (`extract` carries the modality's shape and dimension
    /// rules), then the missing-slot drain.
    fn scatter_batch_response<T>(
        data: Vec<EmbedData>,
        expected: usize,
        code: &'static str,
        count_msg: impl FnOnce(usize, usize) -> String,
        slot_prefix: &'static str,
        missing_msg: impl Fn(usize) -> String,
        mut extract: impl FnMut(usize, EmbeddingPayload) -> Result<T, QqlError>,
    ) -> Result<Vec<T>, QqlError> {
        if data.len() != expected {
            return Err(QqlError::execution(
                code,
                count_msg(data.len(), expected),
                None,
            ));
        }
        let mut slots: Vec<Option<T>> = Vec::new();
        slots.resize_with(expected, || None);
        for item in data {
            if item.index >= expected {
                return Err(QqlError::execution(
                    code,
                    format!("{slot_prefix} response index {} out of range", item.index),
                    None,
                ));
            }
            if slots[item.index].is_some() {
                return Err(QqlError::execution(
                    code,
                    format!("{slot_prefix} response duplicated index {}", item.index),
                    None,
                ));
            }
            slots[item.index] = Some(extract(item.index, item.embedding)?);
        }
        slots
            .into_iter()
            .enumerate()
            .map(|(i, slot)| slot.ok_or_else(|| QqlError::execution(code, missing_msg(i), None)))
            .collect()
    }

    /// Embed a dense batch in one request, validating count, index coverage,
    /// and per-vector dimension against the configured `dimension`.
    pub async fn embed_batch_with_model(
        &self,
        inputs: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        // One HTTP request for the full batch (OpenAI: up to 2048 inputs).
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let model_name = self.resolve_dense_model(model);
        let body = EmbedRequest {
            model: model_name,
            input: inputs,
        };

        let decoded = self
            .do_request(&self.endpoint, &self.api_key, &body)
            .await?;

        let dimension = self.dimension;
        Self::scatter_batch_response(
            decoded.data,
            inputs.len(),
            "QQL-EMBEDDING",
            |got, expected| {
                format!("embedding response returned {got} vector(s) for {expected} input(s)")
            },
            "embedding",
            |i| format!("missing embedding vector at index {i}"),
            |index, payload| {
                let dense = match payload {
                    EmbeddingPayload::Dense(v) => v,
                    EmbeddingPayload::Multi(_) => {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING",
                            format!(
                                "embedding response index {index} returned multivector; expected dense"
                            ),
                            None,
                        ));
                    }
                };
                if dense.len() != dimension {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING",
                        format!(
                            "embedding dimension mismatch for index {index}: got {} want {dimension}",
                            dense.len(),
                        ),
                        None,
                    ));
                }
                Ok(dense)
            },
        )
    }

    /// Embed a dense batch with the configured default model.
    pub async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_batch_with_model(inputs, &self.model).await
    }

    async fn embed_multi_batch_with_model(
        &self,
        inputs: &[String],
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if !self.multi_enabled() {
            return Err(qql_embed::embedder::multi_unsupported_error(model));
        }

        let model_name = self.resolve_multi_model(model);
        let body = EmbedRequest {
            model: model_name,
            input: inputs,
        };

        let decoded = self
            .do_request(self.multi_url(), self.multi_key(), &body)
            .await?;

        let multi_dimension = self.multi_dimension;
        Self::scatter_batch_response(
            decoded.data,
            inputs.len(),
            "QQL-EMBEDDING-MULTI",
            |got, expected| {
                format!(
                    "multi embedding response returned {got} result(s) for {expected} input(s) (model={model_name})"
                )
            },
            "multi embedding",
            |i| format!("missing multi embedding at index {i}"),
            |index, payload| {
                let multi = match payload {
                    EmbeddingPayload::Multi(rows) => rows,
                    EmbeddingPayload::Dense(flat) => {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING-MULTI",
                            format!(
                                "multi embedding endpoint returned a flat dense vector (len={}) for index {index}; \
                                 expected nested array [[f32,…],…] (token-level multivector). \
                                 Point MODEL at a ColBERT multi service or set multi_embedding_endpoint.",
                                flat.len(),
                            ),
                            None,
                        ));
                    }
                };
                if multi.is_empty() {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-MULTI",
                        format!("multi embedding returned empty bag at index {index}"),
                        None,
                    ));
                }
                if multi_dimension > 0 {
                    for (row_i, row) in multi.iter().enumerate() {
                        if row.len() != multi_dimension {
                            return Err(QqlError::execution(
                                "QQL-EMBEDDING-MULTI",
                                format!(
                                    "multi embedding dimension mismatch at index {index} row {row_i}: got {} want {multi_dimension}",
                                    row.len(),
                                ),
                                None,
                            ));
                        }
                    }
                }
                Ok(multi)
            },
        )
    }

    fn resolve_image_model<'a>(&'a self, model: &'a str) -> &'a str {
        if !model.is_empty() && model != "default" {
            model
        } else {
            self.image_model.as_deref().unwrap_or(self.model.as_str())
        }
    }

    fn image_url(&self) -> &str {
        self.image_endpoint
            .as_deref()
            .unwrap_or(self.endpoint.as_str())
    }

    fn image_key(&self) -> &str {
        self.image_api_key
            .as_deref()
            .unwrap_or(self.api_key.as_str())
    }

    fn image_dim(&self) -> usize {
        if self.image_dimension > 0 {
            self.image_dimension
        } else {
            self.dimension
        }
    }

    /// Embed image paths/URLs via OpenAI-compatible dense endpoint.
    ///
    /// Contract: `POST { "model", "input": ["path-or-url", ...] }` → dense
    /// `embedding: [f32,…]` per input (same shape as text). Servers that accept
    /// filesystem paths or HTTP(S) image URLs can back CLIP vision this way.
    async fn embed_image_batch_with_model(
        &self,
        sources: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        if !self.image_enabled() {
            return Err(qql_embed::embedder::image_unsupported_error(model));
        }

        let model_name = self.resolve_image_model(model);
        let body = EmbedRequest {
            model: model_name,
            input: sources,
        };
        let decoded = self
            .do_request(self.image_url(), self.image_key(), &body)
            .await?;

        let want_dim = self.image_dim();
        Self::scatter_batch_response(
            decoded.data,
            sources.len(),
            "QQL-EMBEDDING-IMAGE",
            |got, expected| {
                format!(
                    "image embedding response returned {got} vector(s) for {expected} source(s)"
                )
            },
            "image embedding",
            |i| format!("missing image embedding at index {i}"),
            |index, payload| {
                let dense = match payload {
                    EmbeddingPayload::Dense(v) => v,
                    EmbeddingPayload::Multi(_) => {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING-IMAGE",
                            format!(
                                "image embedding index {index} returned multivector; expected dense (CLIP vision is single-vector)"
                            ),
                            None,
                        ));
                    }
                };
                if dense.len() != want_dim {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-IMAGE",
                        format!(
                            "image embedding dimension mismatch for index {index}: got {} want {want_dim}",
                            dense.len(),
                        ),
                        None,
                    ));
                }
                Ok(dense)
            },
        )
    }
}

#[cfg(feature = "rest")]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Embedder for HttpEmbedder {
    fn dimension(&self) -> Option<usize> {
        Some(self.dimension)
    }

    fn multi_dimension(&self) -> Option<usize> {
        if self.multi_dimension > 0 {
            Some(self.multi_dimension)
        } else {
            None
        }
    }

    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        let results = self
            .embed_batch_with_model(&[text.to_string()], model)
            .await?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_batch_with_model(texts, model).await
    }

    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        self.bm25_pipeline.embed_query(text)
    }

    async fn embed_sparse_query_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.bm25_pipeline.embed_query(text)?);
        }
        Ok(results)
    }

    fn bm25_params(&self) -> Bm25Params {
        self.bm25_text.params
    }

    fn bm25_text_config(&self) -> qql_embed::Bm25TextConfig {
        self.bm25_text.clone()
    }

    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        self.bm25_pipeline.embed_document(text)
    }

    async fn embed_sparse_document_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.bm25_pipeline.embed_document(text)?);
        }
        Ok(results)
    }

    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        let results = self
            .embed_multi_batch_with_model(&[text.to_string()], model)
            .await?;
        results.into_iter().next().ok_or_else(|| {
            QqlError::execution(
                "QQL-EMBEDDING-MULTI",
                "multi embedding response was empty",
                None,
            )
        })
    }

    async fn embed_multi_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
        self.embed_multi_batch_with_model(texts, model).await
    }

    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        let results = self
            .embed_image_batch_with_model(&[source.to_string()], model)
            .await?;
        results.into_iter().next().ok_or_else(|| {
            QqlError::execution(
                "QQL-EMBEDDING-IMAGE",
                "image embedding response was empty",
                None,
            )
        })
    }

    async fn embed_image_batch(
        &self,
        sources: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_image_batch_with_model(sources, model).await
    }

    async fn rerank_pairs(
        &self,
        query: &str,
        documents: &[String],
        model: &str,
    ) -> Result<Vec<f32>, QqlError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        if !self.rerank_enabled() {
            return Err(qql_embed::cross_rerank_unsupported_error(model));
        }
        let endpoint = self
            .rerank_endpoint
            .as_deref()
            .ok_or_else(|| qql_embed::cross_rerank_unsupported_error(model))?;
        let model_name = if !model.is_empty() && model != "default" {
            model.to_string()
        } else {
            self.rerank_model
                .clone()
                .unwrap_or_else(|| "rerank".to_string())
        };
        let api_key = self
            .rerank_api_key
            .as_deref()
            .unwrap_or(self.api_key.as_str());

        // Cohere-compatible: { model, query, documents } → results[{index,relevance_score}]
        let body = serde_json::json!({
            "model": model_name,
            "query": query,
            "documents": documents,
        });
        let mut req = self.client.post(endpoint).json(&body);
        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {api_key}"));
        }
        let resp = req.send().await.map_err(|e| {
            QqlError::execution(
                "QQL-RERANK-CROSS",
                format!("failed to call rerank endpoint: {e}"),
                None,
            )
        })?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS",
                format!("rerank endpoint returned {status}: {text}"),
                None,
            ));
        }
        let value: serde_json::Value = resp.json().await.map_err(|e| {
            QqlError::execution(
                "QQL-RERANK-CROSS",
                format!("failed to decode rerank response: {e}"),
                None,
            )
        })?;
        // Accept results[] with index + relevance_score | score
        let results = value
            .get("results")
            .or_else(|| value.get("data"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                QqlError::execution(
                    "QQL-RERANK-CROSS",
                    "rerank response missing results array",
                    None,
                )
            })?;
        let mut scores = vec![0.0f32; documents.len()];
        let mut seen = vec![false; documents.len()];
        for item in results {
            let idx = item.get("index").and_then(|v| v.as_u64()).ok_or_else(|| {
                QqlError::execution("QQL-RERANK-CROSS", "rerank result missing index", None)
            })? as usize;
            if idx >= documents.len() {
                return Err(QqlError::execution(
                    "QQL-RERANK-CROSS",
                    format!("rerank result index {idx} out of range"),
                    None,
                ));
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
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS",
                "rerank response did not cover all documents",
                None,
            ));
        }
        Ok(scores)
    }
}
