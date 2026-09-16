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
    /// BM25 `k1` override for client-side (local) document sparse embedding;
    /// `None` uses the Qdrant default (`1.2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_k1: Option<f64>,
    /// BM25 `b` override; `None` uses the Qdrant default (`0.75`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_b: Option<f64>,
    /// BM25 average document length override; `None` uses the Qdrant default (`256`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_avg_len: Option<f64>,
    /// BM25 text-processing language (Qdrant name/alias, e.g. `"spanish"`).
    /// `None` keeps English. Drives default stopwords/stemmer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_language: Option<String>,
    /// BM25 tokenizer (`"word"`, `"whitespace"`, `"prefix"`). `None` keeps `"word"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_tokenizer: Option<String>,
    /// Lowercase before matching. `None` keeps `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_lowercase: Option<bool>,
    /// Lucene ASCII folding before lowercasing. `None` keeps `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_ascii_folding: Option<bool>,
    /// Custom stopwords replacing the language default (`None` keeps it;
    /// `Some(vec![])` disables filtering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_stopwords: Option<Vec<String>>,
    /// Stemmer override (`None` = language default; `Some("none")` disables).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_stemmer: Option<String>,
    /// Drop tokens shorter than this (chars). `None` keeps no minimum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_min_token_len: Option<usize>,
    /// Drop over-long tokens on the document path (chars). `None` keeps no maximum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_max_token_len: Option<usize>,
}

impl QqlConfig {
    /// Resolve the configured client-side BM25 document parameters, validating
    /// them fail-closed (`QQL-VALIDATION-CONFIG`) before any document is
    /// written. Unset fields keep the Qdrant `qdrant/bm25` defaults.
    pub fn bm25_params(&self) -> Result<qql_embed::Bm25Params, QqlError> {
        qql_embed::Bm25Params::resolve(self.bm25_k1, self.bm25_b, self.bm25_avg_len)
    }

    /// Resolve the full BM25 text configuration (single choke point over
    /// [`qql_embed::Bm25TextConfig::resolve`]).
    pub fn bm25_text_config(&self) -> Result<qql_embed::Bm25TextConfig, QqlError> {
        qql_embed::Bm25TextConfig::resolve(
            self.bm25_k1,
            self.bm25_b,
            self.bm25_avg_len,
            self.bm25_language.as_deref(),
            self.bm25_tokenizer.as_deref(),
            self.bm25_lowercase,
            self.bm25_ascii_folding,
            self.bm25_stopwords.clone(),
            self.bm25_stemmer.as_deref(),
            self.bm25_min_token_len,
            self.bm25_max_token_len,
        )
    }

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

    /// Persist this config to the config file as pretty JSON with owner-only
    /// permissions (the file may hold the Qdrant API secret).
    pub fn save(&self) -> Result<(), QqlError> {
        let path = Self::config_path()?;
        let data = serde_json::to_string_pretty(self).map_err(|e| {
            QqlError::execution(
                "QQL-CONFIG",
                format!("failed to serialize config: {}", e),
                None,
            )
        })?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::fs::PermissionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| {
                    QqlError::execution(
                        "QQL-CONFIG",
                        format!("failed to write config: {}", e),
                        None,
                    )
                })?;
            file.write_all(data.as_bytes()).map_err(|e| {
                QqlError::execution("QQL-CONFIG", format!("failed to write config: {}", e), None)
            })?;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, &data).map_err(|e| {
                QqlError::execution("QQL-CONFIG", format!("failed to write config: {}", e), None)
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::QqlConfig;

    #[test]
    fn bm25_params_resolve_and_validate_fail_closed() {
        let defaults = QqlConfig::default().bm25_params().expect("defaults valid");
        assert_eq!(defaults.k1(), 1.2);
        assert_eq!(defaults.b(), 0.75);
        assert_eq!(defaults.avg_len(), 256.0);

        let configured = QqlConfig {
            bm25_k1: Some(2.0),
            bm25_b: Some(0.5),
            bm25_avg_len: Some(8.0),
            ..Default::default()
        }
        .bm25_params()
        .expect("valid overrides");
        assert_eq!(configured.k1(), 2.0);
        assert_eq!(configured.b(), 0.5);
        assert_eq!(configured.avg_len(), 8.0);

        for cfg in [
            QqlConfig {
                bm25_k1: Some(-1.0),
                ..Default::default()
            },
            QqlConfig {
                bm25_b: Some(1.5),
                ..Default::default()
            },
            QqlConfig {
                bm25_avg_len: Some(f64::NAN),
                ..Default::default()
            },
        ] {
            let err = cfg.bm25_params().expect_err("must fail closed");
            assert_eq!(err.code, "QQL-VALIDATION-CONFIG");
        }
    }
}
