use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptionsWithLength, TextEmbedding};

use qql::embedder::Embedder;
use qql::sparse::SparseVector;
use qql_core::error::QqlError;

fn err(msg: impl Into<std::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::execution("QQL-EDGE", msg, None)
}

pub struct FastEmbedder {
    model: Arc<Mutex<TextEmbedding>>,
    model_name: String,
}

impl FastEmbedder {
    pub fn try_new(options: InitOptionsWithLength<EmbeddingModel>) -> Result<Self, QqlError> {
        let model_name = format!("{:?}", options.model_name);
        let model = TextEmbedding::try_new(options)
            .map_err(|e| err(format!("fastembed init failed: {e}")))?;
        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            model_name,
        })
    }

    pub fn try_default() -> Result<Self, QqlError> {
        Self::try_new(Default::default())
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}

impl std::fmt::Debug for FastEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastEmbedder")
            .field("model_name", &self.model_name)
            .finish()
    }
}

#[async_trait]
impl Embedder for FastEmbedder {
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        let model = self.model.clone();
        let texts = vec![text.to_string()];

        let mut embeddings = tokio::task::spawn_blocking(move || {
            let mut model = model
                .lock()
                .map_err(|e| err(format!("fastembed mutex poisoned: {e}")))?;
            model
                .embed(texts, None)
                .map_err(|e| err(format!("fastembed failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        embeddings
            .pop()
            .ok_or_else(|| err("fastembed returned empty result"))
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        _model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let model = self.model.clone();
        let batch = texts.to_vec();

        let embeddings = tokio::task::spawn_blocking(move || {
            let mut model = model
                .lock()
                .map_err(|e| err(format!("fastembed mutex poisoned: {e}")))?;
            model
                .embed(batch, None)
                .map_err(|e| err(format!("fastembed batch failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        Ok(embeddings)
    }

    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError> {
        Ok(qql::sparse::build_query_default(text))
    }
}
