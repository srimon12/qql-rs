use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeConfig {
    pub data_dir: PathBuf,
    pub on_disk_payload: bool,
    pub embedder: String,
    pub model: Option<String>,
    /// Offline multivector model for fastembed (e.g. `"bge-m3"`).
    pub multi_model: Option<String>,
    /// Offline CLIP vision model (e.g. `"clip-vision"`).
    pub image_model: Option<String>,
    /// Offline cross-encoder model (e.g. `"bge-reranker-base"`).
    pub reranker_model: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub show_download_progress: bool,
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
            embedder: "fastembed".to_string(),
            model: None,
            multi_model: None,
            image_model: None,
            reranker_model: None,
            cache_dir: None,
            show_download_progress: false,
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
        if let Some(value) = env_string("QQL_EDGE_MULTI_MODEL") {
            self.multi_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_IMAGE_MODEL") {
            self.image_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_RERANKER_MODEL")
            .or_else(|| env_string("RERANK_MODEL"))
        {
            self.reranker_model = Some(value);
        }
        if let Some(value) = env_string("QQL_EDGE_CACHE_DIR") {
            self.cache_dir = Some(PathBuf::from(value));
        }
        if let Some(value) = env_bool("QQL_EDGE_ON_DISK") {
            self.on_disk_payload = value;
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
