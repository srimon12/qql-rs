//! Embedding adapters for the runtime.
//!
//! The shared [`Embedder`] trait and AST resolve live in `qql-embed`.
//! This module re-exports them and provides [`HttpEmbedder`] (reqwest).

#[cfg(feature = "rest")]
use async_trait::async_trait;
#[cfg(feature = "rest")]
use reqwest::Client;
#[cfg(feature = "rest")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rest")]
use qql_core::error::QqlError;

// Re-export shared API so existing `qql::embedder::Embedder` paths keep working.
pub use qql_embed::embedder::{Embedder, EmbedderBound, SparseEmbedder};
pub use qql_embed::SparseVector;

#[cfg(feature = "rest")]
#[derive(Debug, Clone, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
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

/// Options for constructing an [`HttpEmbedder`].
#[cfg(feature = "rest")]
#[derive(Debug, Clone, Default)]
pub struct HttpEmbedderOptions {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub dimension: usize,
    /// Optional multi/ColBERT endpoint. Falls back to `endpoint` when empty.
    pub multi_endpoint: Option<String>,
    pub multi_api_key: Option<String>,
    pub multi_model: Option<String>,
    /// Expected per-token dim for multi responses. `0` skips dim checks.
    pub multi_dimension: usize,
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
    client: Client,
}

#[cfg(feature = "rest")]
impl HttpEmbedder {
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
            multi_endpoint: opts
                .multi_endpoint
                .filter(|s| !s.trim().is_empty()),
            multi_api_key: opts.multi_api_key,
            multi_model: opts
                .multi_model
                .filter(|s| !s.trim().is_empty()),
            multi_dimension: opts.multi_dimension,
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

    pub fn multi_enabled(&self) -> bool {
        self.multi_model.is_some()
            || self.multi_endpoint.is_some()
            || self.multi_dimension > 0
    }

    pub async fn probe_dimension(&self, input: &str) -> Result<usize, QqlError> {
        let body = EmbedRequest {
            model: self.model.clone(),
            input: vec![input.to_string()],
        };

        let resp = self.do_request(&self.endpoint, &self.api_key, &body).await?;

        if resp.data.is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding response contained no vectors",
                None,
            ));
        }

        match &resp.data[0].embedding {
            EmbeddingPayload::Dense(v) => Ok(v.len()),
            EmbeddingPayload::Multi(rows) => rows
                .first()
                .map(Vec::len)
                .ok_or_else(|| {
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
        body: &EmbedRequest,
    ) -> Result<EmbedResponse, QqlError> {
        let mut req = self.client.post(endpoint).json(body);

        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        let resp = req.send().await.map_err(|e| {
            QqlError::execution(
                "QQL-EMBEDDING",
                format!("failed to call embedding endpoint: {}", e),
                None,
            )
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

    fn resolve_dense_model(&self, model: &str) -> String {
        if !model.is_empty() && model != "default" {
            model.to_string()
        } else {
            self.model.clone()
        }
    }

    fn resolve_multi_model(&self, model: &str) -> String {
        if !model.is_empty() && model != "default" {
            return model.to_string();
        }
        self.multi_model
            .clone()
            .unwrap_or_else(|| self.model.clone())
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
            input: inputs.to_vec(),
        };

        let decoded = self
            .do_request(&self.endpoint, &self.api_key, &body)
            .await?;

        if decoded.data.len() != inputs.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                format!(
                    "embedding response returned {} vector(s) for {} input(s)",
                    decoded.data.len(),
                    inputs.len()
                ),
                None,
            ));
        }

        let mut vectors: Vec<Option<Vec<f32>>> = vec![None; inputs.len()];
        for item in decoded.data {
            if item.index >= inputs.len() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("embedding response index {} out of range", item.index),
                    None,
                ));
            }
            if vectors[item.index].is_some() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("embedding response duplicated index {}", item.index),
                    None,
                ));
            }
            let dense = match item.embedding {
                EmbeddingPayload::Dense(v) => v,
                EmbeddingPayload::Multi(_) => {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING",
                        format!(
                            "embedding response index {} returned multivector; expected dense",
                            item.index
                        ),
                        None,
                    ));
                }
            };
            if dense.len() != self.dimension {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    format!(
                        "embedding dimension mismatch for index {}: got {} want {}",
                        item.index,
                        dense.len(),
                        self.dimension
                    ),
                    None,
                ));
            }
            vectors[item.index] = Some(dense);
        }

        let mut result = Vec::with_capacity(vectors.len());
        for (i, v) in vectors.into_iter().enumerate() {
            if let Some(vec) = v {
                result.push(vec);
            } else {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("missing embedding vector at index {}", i),
                    None,
                ));
            }
        }

        Ok(result)
    }

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
            model: model_name.clone(),
            input: inputs.to_vec(),
        };

        let decoded = self
            .do_request(self.multi_url(), self.multi_key(), &body)
            .await?;

        if decoded.data.len() != inputs.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-MULTI",
                format!(
                    "multi embedding response returned {} result(s) for {} input(s) (model={model_name})",
                    decoded.data.len(),
                    inputs.len()
                ),
                None,
            ));
        }

        let mut vectors: Vec<Option<Vec<Vec<f32>>>> = vec![None; inputs.len()];
        for item in decoded.data {
            if item.index >= inputs.len() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-MULTI",
                    format!("multi embedding response index {} out of range", item.index),
                    None,
                ));
            }
            if vectors[item.index].is_some() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-MULTI",
                    format!("multi embedding response duplicated index {}", item.index),
                    None,
                ));
            }
            let multi = match item.embedding {
                EmbeddingPayload::Multi(rows) => rows,
                EmbeddingPayload::Dense(flat) => {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-MULTI",
                        format!(
                            "multi embedding endpoint returned a flat dense vector (len={}) for index {}; \
                             expected nested array [[f32,…],…] (token-level multivector). \
                             Point MODEL at a ColBERT multi service or set multi_embedding_endpoint.",
                            flat.len(),
                            item.index
                        ),
                        None,
                    ));
                }
            };
            if multi.is_empty() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-MULTI",
                    format!("multi embedding returned empty bag at index {}", item.index),
                    None,
                ));
            }
            if self.multi_dimension > 0 {
                for (row_i, row) in multi.iter().enumerate() {
                    if row.len() != self.multi_dimension {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING-MULTI",
                            format!(
                                "multi embedding dimension mismatch at index {} row {}: got {} want {}",
                                item.index,
                                row_i,
                                row.len(),
                                self.multi_dimension
                            ),
                            None,
                        ));
                    }
                }
            }
            vectors[item.index] = Some(multi);
        }

        let mut result = Vec::with_capacity(vectors.len());
        for (i, v) in vectors.into_iter().enumerate() {
            if let Some(vec) = v {
                result.push(vec);
            } else {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-MULTI",
                    format!("missing multi embedding at index {}", i),
                    None,
                ));
            }
        }
        Ok(result)
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

    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError> {
        Ok(qql_embed::sparse::build_query_default(text))
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
}
