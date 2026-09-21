//! Inherent accessors: constructors, model introspection, acceptance checks.
//!
//! Pure move from `embedder.rs` (size hygiene split).

use qql_core::error::QqlError;

use super::catalog::short_alias_matches;
use super::catalog::{is_image_alias, is_multi_alias, is_reranker_alias, is_sparse_alias};
use super::{FastEmbedder, FastEmbedderOptions};

impl FastEmbedder {
    /// Construct with default options (dense `BGESmallENV15`, no sparse/multi/image/rerank).
    pub fn try_default() -> Result<Self, QqlError> {
        Self::try_with_options(FastEmbedderOptions::default())
    }

    /// Build from a dense model name string. See [`super::catalog::resolve_embedding_model`].
    pub fn try_from_model(model: &str) -> Result<Self, QqlError> {
        Self::try_with_options(FastEmbedderOptions {
            model: Some(model.to_string()),
            ..Default::default()
        })
    }

    /// Dense model enum-style name (e.g. `BGESmallENV15`).
    pub fn model_name(&self) -> &str {
        &self.dense.model_name
    }

    /// Dense HuggingFace model code (e.g. `Xenova/bge-small-en-v1.5`).
    pub fn model_code(&self) -> &str {
        &self.dense.model_code
    }

    /// Dense embedding dimension.
    pub fn dimension(&self) -> usize {
        self.dense.dim
    }

    /// Multivector model enum-style name, when configured.
    pub fn multi_model_name(&self) -> Option<&str> {
        self.multi.as_ref().map(|m| m.model_name.as_str())
    }

    /// Multivector model code, when configured.
    pub fn multi_model_code(&self) -> Option<&str> {
        self.multi.as_ref().map(|m| m.model_code.as_str())
    }

    /// Per-token multivector (ColBERT) dimension, when configured.
    pub fn multi_dimension(&self) -> Option<usize> {
        self.multi.as_ref().map(|m| m.dim)
    }

    /// Whether an offline multivector (BGE-M3 ColBERT) model is configured.
    pub fn has_multi(&self) -> bool {
        self.multi.is_some()
    }

    /// Image/CLIP vision model enum-style name, when configured.
    pub fn image_model_name(&self) -> Option<&str> {
        self.image.as_ref().map(|m| m.model_name.as_str())
    }

    /// Image/CLIP vision model code, when configured.
    pub fn image_model_code(&self) -> Option<&str> {
        self.image.as_ref().map(|m| m.model_code.as_str())
    }

    /// Image embedding dimension, when configured.
    pub fn image_dimension(&self) -> Option<usize> {
        self.image.as_ref().map(|m| m.dim)
    }

    /// Whether an offline image (CLIP vision) model is configured.
    pub fn has_image(&self) -> bool {
        self.image.is_some()
    }

    /// Whether an offline cross-encoder reranker is configured.
    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some()
    }

    /// Offline sparse model enum-style name, when configured.
    pub fn sparse_model_name(&self) -> Option<&str> {
        self.sparse.as_ref().map(|s| s.model_name.as_str())
    }

    /// Offline sparse model code, when configured.
    pub fn sparse_model_code(&self) -> Option<&str> {
        self.sparse.as_ref().map(|s| s.model_code.as_str())
    }

    /// Whether an offline sparse (SPLADE / BGE-M3) model is configured;
    /// otherwise sparse requests use wire-compatible BM25.
    pub fn has_sparse(&self) -> bool {
        self.sparse.is_some()
    }

    /// Cross-encoder reranker model code, when configured.
    pub fn reranker_model_code(&self) -> Option<&str> {
        self.reranker.as_ref().map(|m| m.model_code.as_str())
    }

    pub(crate) fn accepts_reranker_model(&self, requested: &str) -> bool {
        let Some(ref r) = self.reranker else {
            return false;
        };
        let req = requested.trim();
        if req.is_empty() || req.eq_ignore_ascii_case("default") {
            return true;
        }
        req.eq_ignore_ascii_case(&r.model_name)
            || req.eq_ignore_ascii_case(&r.model_code)
            || short_alias_matches(req, &r.model_code)
            || is_reranker_alias(req)
    }

    /// Whether a QQL `USING MODEL '…'` / `MODEL '…'` string refers to this embedder.
    /// Empty / `"default"` always match (host did not pin a model).
    pub fn accepts_model(&self, requested: &str) -> bool {
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        if r.eq_ignore_ascii_case(&self.dense.model_name)
            || r.eq_ignore_ascii_case(&self.dense.model_code)
            || short_alias_matches(r, &self.dense.model_code)
        {
            return true;
        }
        if let Some(ref sparse) = self.sparse
            && (r.eq_ignore_ascii_case(&sparse.model_name)
                || r.eq_ignore_ascii_case(&sparse.model_code)
                || short_alias_matches(r, &sparse.model_code)
                || is_sparse_alias(r))
        {
            return true;
        }
        if let Some(ref multi) = self.multi
            && (r.eq_ignore_ascii_case(&multi.model_name)
                || r.eq_ignore_ascii_case(&multi.model_code)
                || short_alias_matches(r, &multi.model_code)
                || is_multi_alias(r))
        {
            return true;
        }
        if let Some(ref image) = self.image
            && (r.eq_ignore_ascii_case(&image.model_name)
                || r.eq_ignore_ascii_case(&image.model_code)
                || short_alias_matches(r, &image.model_code)
                || is_image_alias(r))
        {
            return true;
        }
        false
    }

    pub(crate) fn accepts_image_model(&self, requested: &str) -> bool {
        let Some(ref image) = self.image else {
            return false;
        };
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        r.eq_ignore_ascii_case(&image.model_name)
            || r.eq_ignore_ascii_case(&image.model_code)
            || short_alias_matches(r, &image.model_code)
            || is_image_alias(r)
    }

    pub(crate) fn accepts_dense_model(&self, requested: &str) -> bool {
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        r.eq_ignore_ascii_case(&self.dense.model_name)
            || r.eq_ignore_ascii_case(&self.dense.model_code)
            || short_alias_matches(r, &self.dense.model_code)
    }

    pub(crate) fn accepts_sparse_model(&self, requested: &str) -> bool {
        let Some(ref sparse) = self.sparse else {
            return false;
        };
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        r.eq_ignore_ascii_case(&sparse.model_name)
            || r.eq_ignore_ascii_case(&sparse.model_code)
            || short_alias_matches(r, &sparse.model_code)
            || is_sparse_alias(r)
    }

    pub(crate) fn accepts_multi_model(&self, requested: &str) -> bool {
        let Some(ref multi) = self.multi else {
            return false;
        };
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        r.eq_ignore_ascii_case(&multi.model_name)
            || r.eq_ignore_ascii_case(&multi.model_code)
            || short_alias_matches(r, &multi.model_code)
            || is_multi_alias(r)
    }
}

impl std::fmt::Debug for FastEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastEmbedder")
            .field("model_name", &self.dense.model_name)
            .field("model_code", &self.dense.model_code)
            .field("dim", &self.dense.dim)
            .field(
                "sparse_model",
                &self.sparse.as_ref().map(|m| m.model_code.as_str()),
            )
            .field(
                "multi_model",
                &self.multi.as_ref().map(|m| m.model_code.as_str()),
            )
            .field("multi_dim", &self.multi.as_ref().map(|m| m.dim))
            .field(
                "image_model",
                &self.image.as_ref().map(|m| m.model_code.as_str()),
            )
            .field("image_dim", &self.image.as_ref().map(|m| m.dim))
            .finish()
    }
}
