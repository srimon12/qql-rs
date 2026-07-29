use crate::executor::Executor;

impl Executor {
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
}
