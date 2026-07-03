use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use qql_core::error::QqlError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QqlConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub inference_model: Option<String>,
    #[serde(default)]
    pub sparse_inference_model: Option<String>,
    #[serde(default)]
    pub inference_mode: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cloud_model_options: HashMap<String, String>,
    #[serde(default)]
    pub embedding_endpoint: Option<String>,
    #[serde(default)]
    pub embedding_api_key: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default)]
    pub embedding_dimension: usize,
    #[serde(default)]
    pub no_verify: bool,
    #[serde(default)]
    pub ca_cert: Option<String>,
    #[serde(default)]
    pub request_timeout: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_k1: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_b: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bm25_avg_dl: Option<f64>,
}

impl QqlConfig {
    pub fn config_dir() -> Result<PathBuf, QqlError> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| QqlError::runtime("could not find home directory"))?;
        let dir = PathBuf::from(home).join(".qql");
        std::fs::create_dir_all(&dir)
            .map_err(|e| QqlError::runtime(format!("could not create config directory: {}", e)))?;
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf, QqlError> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    pub fn load() -> Result<Option<Self>, QqlError> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path)
            .map_err(|e| QqlError::runtime(format!("failed to read config: {}", e)))?;
        let config: QqlConfig = serde_json::from_str(&data)
            .map_err(|e| QqlError::runtime(format!("failed to parse config: {}", e)))?;
        Ok(Some(config))
    }

    pub fn save(&self) -> Result<(), QqlError> {
        let path = Self::config_path()?;
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| QqlError::runtime(format!("failed to serialize config: {}", e)))?;
        std::fs::write(&path, data)
            .map_err(|e| QqlError::runtime(format!("failed to write config: {}", e)))?;
        Ok(())
    }
}
