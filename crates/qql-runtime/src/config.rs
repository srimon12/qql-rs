use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use qql_core::error::QqlError;

/// Persistent QQL CLI / SDK configuration, stored as `$HOME/.qql/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QqlConfig {
    /// Qdrant REST base URL (e.g. `http://localhost:6333`).
    #[serde(default)]
    pub url: String,
    /// Qdrant API key sent as auth credential.
    #[serde(default)]
    pub secret: Option<String>,
    /// Name of the active connection profile, when one is set.
    #[serde(default)]
    pub active_profile: Option<String>,
    /// Default local dense embedding model (fastembed name or alias).
    #[serde(default)]
    pub inference_model: Option<String>,
    /// Default offline sparse embedding model (e.g. `splade`, `bge-m3`).
    #[serde(default)]
    pub sparse_inference_model: Option<String>,
    /// Inference mode: `local` (fastembed) or `remote` (OpenAI-compatible HTTP).
    #[serde(default)]
    pub inference_mode: String,
    /// Extra model options forwarded per model to remote (cloud) inference.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cloud_model_options: HashMap<String, String>,
    /// OpenAI-compatible dense embedding endpoint.
    #[serde(default)]
    pub embedding_endpoint: Option<String>,
    /// Bearer token for the dense embedding endpoint.
    #[serde(default)]
    pub embedding_api_key: Option<String>,
    /// Default dense model name sent to the embedding endpoint.
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Expected dense embedding dimension, validated against responses.
    #[serde(default)]
    pub embedding_dimension: usize,
    /// Optional multi-vector / ColBERT embedding endpoint (OpenAI-compatible).
    /// When unset, multi requests reuse [`Self::embedding_endpoint`].
    #[serde(default)]
    pub multi_embedding_endpoint: Option<String>,
    /// Bearer token for the multi/ColBERT embedding endpoint.
    #[serde(default)]
    pub multi_embedding_api_key: Option<String>,
    /// Default model for multivector / late-interaction embeds (RERANK, AS MULTI).
    #[serde(default)]
    pub multi_embedding_model: Option<String>,
    /// Per-token dimension for multivector models (e.g. 96 ColBERT-small, 1024 BGE-M3).
    #[serde(default)]
    pub multi_embedding_dimension: usize,
    /// Optional image / CLIP vision embedding endpoint (OpenAI-compatible dense).
    /// When unset, image requests reuse [`Self::embedding_endpoint`].
    #[serde(default)]
    pub image_embedding_endpoint: Option<String>,
    /// Bearer token for the image/CLIP vision embedding endpoint.
    #[serde(default)]
    pub image_embedding_api_key: Option<String>,
    /// Default vision model (e.g. `Qdrant/clip-ViT-B-32-vision`).
    #[serde(default)]
    pub image_embedding_model: Option<String>,
    /// Dense dim for image embeds (CLIP ViT-B/32 = 512).
    #[serde(default)]
    pub image_embedding_dimension: usize,
    /// Cross-encoder / pair-rerank HTTP endpoint (Cohere-style or compatible).
    #[serde(default)]
    pub rerank_endpoint: Option<String>,
    /// Bearer token for the cross-encoder rerank endpoint.
    #[serde(default)]
    pub rerank_api_key: Option<String>,
    /// Default cross-encoder model (e.g. `BAAI/bge-reranker-base`).
    #[serde(default)]
    pub rerank_model: Option<String>,
    /// Skip TLS certificate verification (insecure; for local testing).
    #[serde(default)]
    pub no_verify: bool,
    /// Path to a custom CA certificate bundle for TLS.
    #[serde(default)]
    pub ca_cert: Option<String>,
    /// Per-request timeout in seconds; `0` disables the timeout.
    #[serde(default)]
    pub request_timeout: u64,
    /// BM25 `k1` override; `None` uses the Qdrant default (`1.2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_k1: Option<f64>,
    /// BM25 `b` override; `None` uses the Qdrant default (`0.75`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_b: Option<f64>,
    /// BM25 average document length override; `None` uses the Qdrant default (`256`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_avg_dl: Option<f64>,
}

impl QqlConfig {
    /// Ensure and return the QQL config directory (`$HOME/.qql`).
    pub fn config_dir() -> Result<PathBuf, QqlError> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                QqlError::execution("QQL-CONFIG", "could not find home directory", None)
            })?;
        let dir = PathBuf::from(home).join(".qql");
        std::fs::create_dir_all(&dir).map_err(|e| {
            QqlError::execution(
                "QQL-CONFIG",
                format!("could not create config directory: {}", e),
                None,
            )
        })?;
        Ok(dir)
    }

    /// Path of the QQL config file inside the config directory.
    pub fn config_path() -> Result<PathBuf, QqlError> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    /// Load the persisted config; returns `None` when no config file exists.
    pub fn load() -> Result<Option<Self>, QqlError> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path).map_err(|e| {
            QqlError::execution("QQL-CONFIG", format!("failed to read config: {}", e), None)
        })?;
        let config: QqlConfig = serde_json::from_str(&data).map_err(|e| {
            QqlError::execution("QQL-CONFIG", format!("failed to parse config: {}", e), None)
        })?;
        Ok(Some(config))
    }

    /// Persist this config to the config file as pretty JSON.
    pub fn save(&self) -> Result<(), QqlError> {
        let path = Self::config_path()?;
        let data = serde_json::to_string_pretty(self).map_err(|e| {
            QqlError::execution(
                "QQL-CONFIG",
                format!("failed to serialize config: {}", e),
                None,
            )
        })?;
        std::fs::write(&path, data).map_err(|e| {
            QqlError::execution("QQL-CONFIG", format!("failed to write config: {}", e), None)
        })?;
        Ok(())
    }
}
