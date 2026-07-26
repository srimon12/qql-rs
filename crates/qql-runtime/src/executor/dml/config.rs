use std::collections::HashMap;

use crate::executor::Executor;

impl Executor {
    #[allow(dead_code)]
    pub(crate) fn resolve_dense_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if !cfg.embedding_model.as_deref().unwrap_or("").is_empty() {
                return cfg.embedding_model.as_ref().unwrap().clone();
            }
            if !cfg.inference_model.as_deref().unwrap_or("").is_empty() {
                return cfg.inference_model.as_ref().unwrap().clone();
            }
        }
        crate::executor::DENSE_MODEL_DEFAULT.to_string()
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_sparse_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if let Some(ref sm) = cfg.sparse_inference_model {
                if !sm.is_empty() {
                    return sm.clone();
                }
            }
        }
        crate::executor::SPARSE_MODEL_DEFAULT.to_string()
    }

    pub(crate) fn inference_mode(&self) -> String {
        if let Some(ref cfg) = self.config {
            let mode = cfg.inference_mode.trim();
            if !mode.is_empty() {
                return mode.to_lowercase();
            }
        }
        crate::executor::INFERENCE_MODE_DEFAULT.to_string()
    }

    pub(crate) fn uses_local_embeddings(&self) -> bool {
        let mode = self.inference_mode();
        mode == "local" || mode == "external"
    }

    #[allow(dead_code)]
    pub(crate) fn cloud_model_options(&self) -> HashMap<String, String> {
        if self.uses_local_embeddings() {
            return HashMap::new();
        }
        self.config
            .as_ref()
            .map(|c| c.cloud_model_options.clone())
            .unwrap_or_default()
    }
}
