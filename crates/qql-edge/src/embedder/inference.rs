//! Trait dispatch: `Embedder` implementation over the configured slots.
//!
//! Pure move from `embedder.rs` (size hygiene split). This file holds the
//! single `impl Embedder for FastEmbedder` block — trait coherence forbids
//! splitting it further, so it is the one file that stays just over 400
//! lines; every other module in this directory is under the limit.

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_embed::{Bm25Params, Bm25TextConfig, Embedder, JointEmbeddingOutput, SparseVector};

use super::FastEmbedder;
use super::bm25::{embed_sparse_fastembed_batch, ensure_sparse_model_allowed, to_qql_sparse};
use super::err;

#[async_trait]
impl Embedder for FastEmbedder {
    fn dimension(&self) -> Option<usize> {
        Some(self.dense.dim)
    }

    fn multi_dimension(&self) -> Option<usize> {
        self.multi.as_ref().map(|m| m.dim)
    }

    fn accepts_model(&self, model: &str) -> bool {
        self.accepts_model(model)
    }

    /// Document-side BM25 hyperparameters the fallback encoder was built with.
    /// When a fastembed sparse model is configured, sparse inference is
    /// ONNX-backed and these values are not used.
    fn bm25_params(&self) -> Bm25Params {
        self.bm25_params
    }

    /// Full text configuration the fallback encoder was built with. The
    /// embedding itself runs in Qdrant's engine; this mirror keeps estimators
    /// and default-path consumers on the same language/tokenizer.
    fn bm25_text_config(&self) -> Bm25TextConfig {
        self.bm25_text.clone()
    }

    // image_dimension is not on Embedder trait; use dimension() for dense CLIP text.
    // Image dim available via FastEmbedder::image_dimension().

    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        if !self.accepts_dense_model(model) {
            // Multi-only model id on dense path is a clear mistake.
            if self.accepts_multi_model(model) {
                return Err(err(format!(
                    "model '{model}' is the multi/ColBERT model on this edge embedder; \
                     use it with AS MULTI / multivector RERANK, not dense embedding. \
                     Dense model is '{}' ({}).",
                    self.dense.model_name, self.dense.model_code
                )));
            }
            return Err(err(format!(
                "local embedder is locked to dense model '{}' ({}); cannot satisfy USING MODEL '{model}'. \
                 Create the executor with model='{model}' (or omit MODEL to use the locked one).",
                self.dense.model_name, self.dense.model_code
            )));
        }

        let model = self.dense.model.clone();
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
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        if !self.accepts_dense_model(model) {
            if self.accepts_multi_model(model) {
                return Err(err(format!(
                    "model '{model}' is the multi/ColBERT model on this edge embedder; \
                     use it with AS MULTI / multivector RERANK, not dense embedding. \
                     Dense model is '{}' ({}).",
                    self.dense.model_name, self.dense.model_code
                )));
            }
            return Err(err(format!(
                "local embedder is locked to dense model '{}' ({}); cannot satisfy USING MODEL '{model}'. \
                 Create the executor with model='{model}' (or omit MODEL to use the locked one).",
                self.dense.model_name, self.dense.model_code
            )));
        }

        let model = self.dense.model.clone();
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

    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        ensure_sparse_model_allowed(self, model)?;
        if self.sparse.is_some() {
            let mut out = embed_sparse_fastembed_batch(self, vec![text.to_string()]).await?;
            return out
                .pop()
                .ok_or_else(|| err("fastembed SparseTextEmbedding returned no result"));
        }
        Ok(to_qql_sparse(self.bm25.embed_query(text)))
    }

    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        ensure_sparse_model_allowed(self, model)?;
        if self.sparse.is_some() {
            let mut out = embed_sparse_fastembed_batch(self, vec![text.to_string()]).await?;
            return out
                .pop()
                .ok_or_else(|| err("fastembed SparseTextEmbedding returned no result"));
        }
        Ok(to_qql_sparse(self.bm25.embed_document(text)))
    }

    async fn embed_sparse_document_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        ensure_sparse_model_allowed(self, model)?;
        if self.sparse.is_some() {
            return embed_sparse_fastembed_batch(self, texts.to_vec()).await;
        }
        Ok(texts
            .iter()
            .map(|text| to_qql_sparse(self.bm25.embed_document(text)))
            .collect())
    }

    /// Single-pass BGE-M3 joint embedding: one `Bgem3Embedding::embed` call
    /// yields dense, sparse, and ColBERT together. Falls back to the default
    /// three-call implementation when no BGE-M3 multi model is configured.
    async fn embed_joint(&self, text: &str, model: &str) -> Result<JointEmbeddingOutput, QqlError> {
        let Some(ref multi) = self.multi else {
            // No BGE-M3 model: delegate to default per-call impl (non-optimal
            // but correct — no error suppression).
            let dense = self.embed_dense(text, model).await?;
            let sparse = self.embed_sparse_document(text, model).await?;
            let multi_vec = self.embed_multi(text, model).await?;
            return Ok(JointEmbeddingOutput {
                dense: Some(dense),
                sparse: Some(sparse),
                multi: Some(multi_vec),
            });
        };

        if !(self.accepts_multi_model(model)
            || model.is_empty()
            || model.eq_ignore_ascii_case("default"))
        {
            return Err(err(format!(
                "local joint embedder uses BGE-M3 '{}' ({}); cannot satisfy MODEL '{model}'",
                multi.model_name, multi.model_code
            )));
        }

        let model_arc = multi.model.clone();
        let texts = vec![text.to_string()];

        let output = tokio::task::spawn_blocking(move || {
            let mut m = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed joint mutex poisoned: {e}")))?;
            m.embed(texts, None)
                .map_err(|e| err(format!("fastembed BGE-M3 joint failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        let dense = output.dense.into_iter().next();
        let sparse = output.sparse.into_iter().next().map(|e| SparseVector {
            indices: e.indices.iter().map(|&i| i as u32).collect(),
            values: e.values.clone(),
        });
        let colbert = output.colbert.into_iter().next();

        Ok(JointEmbeddingOutput {
            dense,
            sparse,
            multi: colbert,
        })
    }

    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        let Some(ref multi) = self.multi else {
            return Err(qql_embed::multi_unsupported_error(model));
        };
        if !self.accepts_multi_model(model) && !self.accepts_dense_model(model) {
            // Accept dense model id only when multi is configured and model is defaulted;
            // explicit wrong model still errors.
            if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
                return Err(err(format!(
                    "local multi embedder is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
                    multi.model_name, multi.model_code
                )));
            }
        }

        let model_arc = multi.model.clone();
        let texts = vec![text.to_string()];

        let mut output = tokio::task::spawn_blocking(move || {
            let mut model = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed multi mutex poisoned: {e}")))?;
            model
                .embed(texts, None)
                .map_err(|e| err(format!("fastembed multi (BGE-M3) failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        output
            .colbert
            .pop()
            .filter(|rows| !rows.is_empty())
            .ok_or_else(|| err("fastembed multi returned empty ColBERT bag"))
    }

    async fn embed_multi_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let Some(ref multi) = self.multi else {
            return Err(qql_embed::multi_unsupported_error(model));
        };
        if !(self.accepts_multi_model(model)
            || model.is_empty()
            || model.eq_ignore_ascii_case("default"))
        {
            return Err(err(format!(
                "local multi embedder is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
                multi.model_name, multi.model_code
            )));
        }

        let model_arc = multi.model.clone();
        let batch = texts.to_vec();

        let output = tokio::task::spawn_blocking(move || {
            let mut model = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed multi mutex poisoned: {e}")))?;
            model
                .embed(batch, None)
                .map_err(|e| err(format!("fastembed multi batch failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        if output.colbert.iter().any(|rows| rows.is_empty()) {
            return Err(err("fastembed multi returned an empty ColBERT bag"));
        }
        Ok(output.colbert)
    }

    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        let Some(ref image) = self.image else {
            return Err(qql_embed::image_unsupported_error(model));
        };
        if !(self.accepts_image_model(model)
            || model.is_empty()
            || model.eq_ignore_ascii_case("default"))
        {
            return Err(err(format!(
                "local image embedder is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
                image.model_name, image.model_code
            )));
        }

        let model_arc = image.model.clone();
        let path = source.to_string();

        let mut embeddings = tokio::task::spawn_blocking(move || {
            let mut model = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed image mutex poisoned: {e}")))?;
            model
                .embed(vec![path], None)
                .map_err(|e| err(format!("fastembed image embed failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        embeddings
            .pop()
            .ok_or_else(|| err("fastembed image returned empty result"))
    }

    async fn embed_image_batch(
        &self,
        sources: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        if sources.is_empty() {
            return Ok(vec![]);
        }
        let Some(ref image) = self.image else {
            return Err(qql_embed::image_unsupported_error(model));
        };
        if !(self.accepts_image_model(model)
            || model.is_empty()
            || model.eq_ignore_ascii_case("default"))
        {
            return Err(err(format!(
                "local image embedder is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
                image.model_name, image.model_code
            )));
        }

        let model_arc = image.model.clone();
        let batch = sources.to_vec();

        let embeddings = tokio::task::spawn_blocking(move || {
            let mut model = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed image mutex poisoned: {e}")))?;
            model
                .embed(batch, None)
                .map_err(|e| err(format!("fastembed image batch failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        Ok(embeddings)
    }

    async fn rerank_pairs(
        &self,
        query: &str,
        documents: &[String],
        model: &str,
    ) -> Result<Vec<f32>, QqlError> {
        if documents.is_empty() {
            return Ok(vec![]);
        }
        let Some(ref reranker) = self.reranker else {
            return Err(qql_embed::cross_rerank_unsupported_error(model));
        };
        if !(self.accepts_reranker_model(model)
            || model.is_empty()
            || model.eq_ignore_ascii_case("default"))
        {
            return Err(err(format!(
                "local reranker is locked to '{}' ({}); cannot satisfy MODEL '{model}'",
                reranker.model_name, reranker.model_code
            )));
        }

        let model_arc = reranker.model.clone();
        let q = query.to_string();
        let docs = documents.to_vec();

        let ranked = tokio::task::spawn_blocking(move || {
            let mut model = model_arc
                .lock()
                .map_err(|e| err(format!("fastembed rerank mutex poisoned: {e}")))?;
            model
                .rerank(q, docs, false, None)
                .map_err(|e| err(format!("fastembed TextRerank failed: {e}")))
        })
        .await
        .map_err(|e| err(format!("spawn_blocking failed: {e}")))??;

        // Unpermute to original document order.
        let mut scores = vec![0.0f32; documents.len()];
        for item in ranked {
            if item.index < scores.len() {
                scores[item.index] = item.score;
            }
        }
        Ok(scores)
    }
}
