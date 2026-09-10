use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeConfig {
    pub data_dir: PathBuf,
    pub on_disk_payload: bool,
    /// WAL segment capacity in MiB for local edge shards. `None` keeps the
    /// qdrant-edge default (32 MiB pre-allocated per segment). Also exposed by
    /// the Python/Node edge SDKs; qdrant-edge 0.8's own Python binding cannot
    /// set it.
    pub wal_segment_mb: Option<u64>,
    pub embedder: String,
    pub model: Option<String>,
    /// Offline sparse model for fastembed (e.g. `"splade"`, `"bge-m3"`).
    pub sparse_model: Option<String>,
    /// Offline multivector model for fastembed (e.g. `"bge-m3"`).
    pub multi_model: Option<String>,
    /// Offline CLIP vision model (e.g. `"clip-vision"`).
    pub image_model: Option<String>,
    /// Offline cross-encoder model (e.g. `"bge-reranker-base"`).
    pub reranker_model: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub show_download_progress: bool,
    /// Client-side BM25 `k1` for the local wire-compatible document encoder
    /// (used when no offline sparse model is configured). `None` → Qdrant
    /// `qdrant/bm25` default (`1.2`). Write-path only; invalid values fail
    /// closed with `QQL-VALIDATION-CONFIG`.
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b` (`[0, 1]`); `None` → `0.75`.
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length in tokens;
    /// `None` → `256`.
    pub bm25_avg_len: Option<f64>,
    pub embed_url: Option<String>,
    pub embed_key: String,
    pub embed_model: String,
    pub embed_dimension: usize,
    pub multi_embed_url: Option<String>,
    pub multi_embed_key: Option<String>,
    pub multi_embed_model: Option<String>,
    pub multi_embed_dimension: usize,
    pub image_embed_url: Option<String>,
    pub image_embed_key: Option<String>,
    pub image_embed_model: Option<String>,
    pub image_embed_dimension: usize,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        let data_dir = qql::config::QqlConfig::config_dir()
            .unwrap_or_else(|_| PathBuf::from(".qql"))
            .join("edge-data");
        Self {
            data_dir,
            on_disk_payload: true,
            wal_segment_mb: None,
            embedder: "fastembed".to_string(),
            model: None,
            sparse_model: None,
            multi_model: None,
            image_model: None,
            reranker_model: None,
            cache_dir: None,
            show_download_progress: false,
            bm25_k1: None,
            bm25_b: None,
            bm25_avg_len: None,
            embed_url: None,
            embed_key: String::new(),
            embed_model: "nomic-embed-text".to_string(),
            embed_dimension: 768,
            multi_embed_url: None,
            multi_embed_key: None,
            multi_embed_model: None,
            multi_embed_dimension: 0,
            image_embed_url: None,
            image_embed_key: None,
            image_embed_model: None,
            image_embed_dimension: 0,
        }
    }
}

impl EdgeConfig {
    pub fn path() -> Result<PathBuf, qql_core::error::QqlError> {
        Ok(qql::config::QqlConfig::config_dir()?.join("edge.json"))
    }

    #[cfg(feature = "edge")]
    pub fn load() -> Result<Self, qql_core::error::QqlError> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(&path).map_err(|error| {
            qql_core::error::QqlError::execution(
                "QQL-EDGE-CONFIG",
                format!("failed to read {}: {error}", path.display()),
                None,
            )
        })?;
        serde_json::from_str(&source).map_err(|error| {
            qql_core::error::QqlError::execution(
                "QQL-EDGE-CONFIG",
                format!("failed to parse {}: {error}", path.display()),
                None,
            )
        })
    }

    pub fn save(&self) -> Result<PathBuf, qql_core::error::QqlError> {
        let path = Self::path()?;
        let source = serde_json::to_string_pretty(self).map_err(|error| {
            qql_core::error::QqlError::execution(
                "QQL-EDGE-CONFIG",
                format!("failed to serialize edge configuration: {error}"),
                None,
            )
        })?;
        std::fs::write(&path, source).map_err(|error| {
            qql_core::error::QqlError::execution(
                "QQL-EDGE-CONFIG",
                format!("failed to write {}: {error}", path.display()),
                None,
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    qql_core::error::QqlError::execution(
                        "QQL-EDGE-CONFIG",
                        format!("failed to protect {}: {error}", path.display()),
                        None,
                    )
                },
            )?;
        }
        Ok(path)
    }

    #[cfg(feature = "edge")]
    pub fn apply_environment(mut self) -> Self {
        if let Some(value) = env_string("QQL_EDGE_DATA_DIR") {
            self.data_dir = PathBuf::from(value);
        }
        if let Some(value) = env_string("QQL_EDGE_EMBEDDER") {
            self.embedder = value;
        }
        if let Some(value) = env_string("QQL_EDGE_MODEL") {
            self.model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_SPARSE_MODEL") {
            self.sparse_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_MULTI_MODEL") {
            self.multi_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_IMAGE_MODEL") {
            self.image_model = Some(value);
        }
        if let Some(value) =
            env_string("QQL_EDGE_RERANKER_MODEL").or_else(|| env_string("RERANK_MODEL"))
        {
            self.reranker_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_CACHE_DIR") {
            self.cache_dir = Some(PathBuf::from(value));
        }
        if let Some(value) = env_f64("QQL_EDGE_BM25_K1") {
            self.bm25_k1 = Some(value);
        }
        if let Some(value) = env_f64("QQL_EDGE_BM25_B") {
            self.bm25_b = Some(value);
        }
        if let Some(value) = env_f64("QQL_EDGE_BM25_AVG_LEN") {
            self.bm25_avg_len = Some(value);
        }
        if let Some(value) = env_bool("QQL_EDGE_ON_DISK") {
            self.on_disk_payload = value;
        }
        if let Some(value) = env_usize("QQL_EDGE_WAL_SEGMENT_MB") {
            self.wal_segment_mb = Some(value as u64);
        }
        if let Some(value) = env_string("EMBED_URL") {
            self.embed_url = Some(value);
        }
        if let Some(value) = env_string("EMBED_KEY") {
            self.embed_key = value;
        }
        if let Some(value) = env_string("EMBED_MODEL") {
            self.embed_model = value;
        }
        if let Some(value) = env_usize("EMBED_DIM") {
            self.embed_dimension = value;
        }
        if let Some(value) = env_string("MULTI_EMBED_URL") {
            self.multi_embed_url = Some(value);
        }
        if let Some(value) = env_string("MULTI_EMBED_KEY") {
            self.multi_embed_key = Some(value);
        }
        if let Some(value) = env_string("MULTI_EMBED_MODEL") {
            self.multi_embed_model = Some(value.clone());
            if self.multi_model.is_none() {
                self.multi_model = Some(value);
            }
        }
        if let Some(value) = env_usize("MULTI_EMBED_DIM") {
            self.multi_embed_dimension = value;
        }
        if let Some(value) = env_string("IMAGE_EMBED_URL") {
            self.image_embed_url = Some(value);
        }
        if let Some(value) = env_string("IMAGE_EMBED_KEY") {
            self.image_embed_key = Some(value);
        }
        if let Some(value) = env_string("IMAGE_EMBED_MODEL") {
            self.image_embed_model = Some(value.clone());
            if self.image_model.is_none() {
                self.image_model = Some(value);
            }
        }
        if let Some(value) = env_usize("IMAGE_EMBED_DIM") {
            self.image_embed_dimension = value;
        }
        self
    }
}

#[cfg(feature = "edge")]
fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(feature = "edge")]
fn env_bool(name: &str) -> Option<bool> {
    let value = env_string(name)?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(feature = "edge")]
fn env_usize(name: &str) -> Option<usize> {
    env_string(name)?.parse().ok()
}

/// Parse a float env override, mapping malformed values to `NaN` so they fail
/// closed in the BM25 validator (`QQL-VALIDATION-CONFIG`) instead of being
/// silently ignored.
#[cfg(feature = "edge")]
fn env_f64(name: &str) -> Option<f64> {
    parse_bm25_env(env_string(name))
}

#[cfg(feature = "edge")]
fn parse_bm25_env(value: Option<String>) -> Option<f64> {
    Some(value?.parse().unwrap_or(f64::NAN))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_config_default_has_no_sparse_model() {
        let cfg = EdgeConfig::default();
        assert!(cfg.sparse_model.is_none());
    }

    #[test]
    fn edge_config_with_sparse_model() {
        let cfg = EdgeConfig {
            sparse_model: Some("splade".into()),
            ..Default::default()
        };
        assert_eq!(cfg.sparse_model.as_deref(), Some("splade"));
    }

    #[test]
    fn edge_config_default_keeps_other_models_none() {
        let cfg = EdgeConfig::default();
        assert!(cfg.model.is_none());
        assert!(cfg.sparse_model.is_none());
        assert!(cfg.multi_model.is_none());
        assert!(cfg.image_model.is_none());
        assert!(cfg.reranker_model.is_none());
    }

    #[test]
    fn edge_config_default_keeps_bm25_unset() {
        let cfg = EdgeConfig::default();
        assert_eq!(cfg.bm25_k1, None);
        assert_eq!(cfg.bm25_b, None);
        assert_eq!(cfg.bm25_avg_len, None);
    }

    #[test]
    fn edge_config_bm25_values_roundtrip_through_json() {
        let cfg = EdgeConfig {
            bm25_k1: Some(2.0),
            bm25_b: Some(0.5),
            bm25_avg_len: Some(8.0),
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: EdgeConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.bm25_k1, Some(2.0));
        assert_eq!(back.bm25_b, Some(0.5));
        assert_eq!(back.bm25_avg_len, Some(8.0));
    }

    #[test]
    #[cfg(feature = "edge")]
    fn parse_bm25_env_malformed_values_fail_closed_as_nan() {
        // Malformed numbers must not be silently dropped: NaN fails the BM25
        // validator so the executor refuses to start instead.
        assert!(
            parse_bm25_env(Some("not-a-number".into()))
                .unwrap()
                .is_nan()
        );
        assert!(parse_bm25_env(Some(f64::NAN.to_string())).unwrap().is_nan());
        assert_eq!(parse_bm25_env(Some("2.5".into())), Some(2.5));
        assert_eq!(parse_bm25_env(None), None);
    }
}
