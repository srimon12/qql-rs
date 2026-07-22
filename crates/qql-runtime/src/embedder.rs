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

#[cfg(feature = "rest")]
#[derive(Debug, Clone, Deserialize)]
struct EmbedData {
    index: usize,
    embedding: Vec<f32>,
}

/// OpenAI-compatible HTTP embedder (`POST {"model","input":[...]}`).
///
/// Endpoint is **required** — no default URL. Works with OpenAI, Ollama
/// `/v1/embeddings`, Cohere compatibility API, etc. Always batches in one request.
#[cfg(feature = "rest")]
pub struct HttpEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dimension: usize,
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
        if endpoint.trim().is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding endpoint is required",
                None,
            ));
        }
        if model.trim().is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding model is required",
                None,
            ));
        }
        if dimension == 0 {
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
            endpoint,
            api_key,
            model,
            dimension,
            client,
        })
    }

    pub async fn probe_dimension(&self, input: &str) -> Result<usize, QqlError> {
        let body = EmbedRequest {
            model: self.model.clone(),
            input: vec![input.to_string()],
        };

        let resp = self.do_request(&body).await?;

        if resp.data.is_empty() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                "embedding response contained no vectors",
                None,
            ));
        }

        Ok(resp.data[0].embedding.len())
    }

    async fn do_request(&self, body: &EmbedRequest) -> Result<EmbedResponse, QqlError> {
        let mut req = self.client.post(&self.endpoint).json(body);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
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

    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, QqlError> {
        // One HTTP request for the full batch (OpenAI: up to 2048 inputs).
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let body = EmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };

        let decoded = self.do_request(&body).await?;

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
            if item.embedding.len() != self.dimension {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    format!(
                        "embedding dimension mismatch for index {}: got {} want {}",
                        item.index,
                        item.embedding.len(),
                        self.dimension
                    ),
                    None,
                ));
            }
            vectors[item.index] = Some(item.embedding);
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
}

#[cfg(feature = "rest")]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Embedder for HttpEmbedder {
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        _model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_batch(texts).await
    }

    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError> {
        Ok(qql_embed::sparse::build_query_default(text))
    }
}
