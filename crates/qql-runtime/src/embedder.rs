use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use qql_core::error::QqlError;

use crate::sparse::{self, SparseVector};

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError>;
    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError>;
}

#[derive(Debug, Clone, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbedData {
    index: usize,
    embedding: Vec<f32>,
}

pub struct HttpEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dimension: usize,
    client: Client,
}

impl HttpEmbedder {
    pub fn new(
        endpoint: String,
        api_key: String,
        model: String,
        dimension: usize,
    ) -> Result<Self, QqlError> {
        if endpoint.trim().is_empty() {
            return Err(QqlError::runtime("embedding endpoint is required"));
        }
        if model.trim().is_empty() {
            return Err(QqlError::runtime("embedding model is required"));
        }
        if dimension == 0 {
            return Err(QqlError::runtime("embedding dimension must be positive"));
        }

        let client = Client::builder()
            .build()
            .map_err(|e| QqlError::runtime(format!("failed to create HTTP client: {}", e)))?;

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
            return Err(QqlError::runtime("embedding response contained no vectors"));
        }

        Ok(resp.data[0].embedding.len())
    }

    async fn do_request(&self, body: &EmbedRequest) -> Result<EmbedResponse, QqlError> {
        let mut req = self.client.post(&self.endpoint).json(body);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req
            .send()
            .await
            .map_err(|e| QqlError::runtime(format!("failed to call embedding endpoint: {}", e)))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(QqlError::runtime(format!(
                "embedding endpoint returned {}: {}",
                status, text
            )));
        }

        let decoded: EmbedResponse = resp.json().await.map_err(|e| {
            QqlError::runtime(format!("failed to decode embedding response: {}", e))
        })?;

        Ok(decoded)
    }

    async fn embed_batch(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, QqlError> {
        if inputs.is_empty() {
            return Err(QqlError::runtime("inputs are required"));
        }

        let body = EmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };

        let decoded = self.do_request(&body).await?;

        if decoded.data.len() != inputs.len() {
            return Err(QqlError::runtime(format!(
                "embedding response returned {} vector(s) for {} input(s)",
                decoded.data.len(),
                inputs.len()
            )));
        }

        let mut vectors: Vec<Option<Vec<f32>>> = vec![None; inputs.len()];
        for item in decoded.data {
            if item.index >= inputs.len() {
                return Err(QqlError::runtime(format!(
                    "embedding response index {} out of range",
                    item.index
                )));
            }
            if vectors[item.index].is_some() {
                return Err(QqlError::runtime(format!(
                    "embedding response duplicated index {}",
                    item.index
                )));
            }
            if item.embedding.len() != self.dimension {
                return Err(QqlError::runtime(format!(
                    "embedding dimension mismatch for index {}: got {} want {}",
                    item.index,
                    item.embedding.len(),
                    self.dimension
                )));
            }
            vectors[item.index] = Some(item.embedding);
        }

        let mut result = Vec::with_capacity(vectors.len());
        for (i, v) in vectors.into_iter().enumerate() {
            if let Some(vec) = v {
                result.push(vec);
            } else {
                return Err(QqlError::runtime(format!(
                    "missing embedding vector at index {}",
                    i
                )));
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl Embedder for HttpEmbedder {
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    async fn embed_sparse(&self, _text: &str) -> Result<SparseVector, QqlError> {
        Err(QqlError::runtime(
            "HttpEmbedder does not support sparse embedding; use SparseEmbedder",
        ))
    }
}

pub struct SparseEmbedder;

impl SparseEmbedder {
    pub async fn embed_sparse(text: &str) -> SparseVector {
        sparse::build_query_default(text)
    }
}
