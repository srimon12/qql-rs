//! Constructors: option resolution, per-slot model init with shared caches.
//!
//! Pure move from `embedder.rs` (size hygiene split). Model init goes through
//! [`super::cache::load_cached`], which shares one ONNX session per
//! `(model, cache dir)` and loads it at most once under construction races.

use fastembed::{
    Bgem3Embedding, Bgem3InitOptions, EmbeddingModel, ImageEmbedding, ImageInitOptions,
    InitOptionsWithLength, RerankInitOptions, SparseInitOptions, SparseTextEmbedding,
    TextEmbedding, TextRerank,
};
use qdrant_edge::bm25_embed::EdgeBm25;
use qql_core::error::QqlError;
use qql_embed::Bm25TextConfig;

use super::bm25::edge_bm25_config;
use super::catalog::{
    resolve_embedding_model, resolve_image_model, resolve_multi_model, resolve_reranker_model,
    resolve_sparse_model,
};
use super::{
    DenseSlot, FastEmbedder, FastEmbedderOptions, ImageSlot, MultiSlot, RerankSlot, SparseSlot,
    cache_dir_key, dense_cache, err, image_cache, load_cached, multi_cache, rerank_cache,
    sparse_cache,
};

impl FastEmbedder {
    /// Compatibility constructor from a fastembed `InitOptionsWithLength`
    /// (dense model only; no sparse/multi/image/rerank slots).
    ///
    /// Like [`Self::try_with_options`], this downloads and loads the model
    /// before returning. The model is resolved through its stable fastembed
    /// `model_code` rather than its `Debug` rendering, so an enum rename
    /// cannot silently point this constructor at a different model.
    pub fn try_new(options: InitOptionsWithLength<EmbeddingModel>) -> Result<Self, QqlError> {
        let model_code = TextEmbedding::get_model_info(&options.model_name)
            .map_err(|e| err(format!("fastembed has no model info: {e}")))?
            .model_code
            .clone();
        let cache_dir = if options.cache_dir != std::path::PathBuf::new() {
            Some(options.cache_dir.clone())
        } else {
            None
        };
        Self::try_with_options(FastEmbedderOptions {
            model: Some(model_code),
            sparse_model: None,
            multi_model: None,
            image_model: None,
            reranker_model: None,
            cache_dir,
            show_download_progress: options.show_download_progress,
            bm25_k1: None,
            bm25_b: None,
            bm25_avg_len: None,
            bm25_language: None,
            bm25_tokenizer: None,
            bm25_lowercase: None,
            bm25_ascii_folding: None,
            bm25_min_token_len: None,
            bm25_max_token_len: None,
            bm25_stopwords: None,
            bm25_stemmer: None,
            bm25_stopwords_languages: None,
        })
    }

    /// Construct from high-level options (dense + optional sparse/multi/image/
    /// reranker, cache, progress).
    ///
    /// Every configured slot is downloaded (when not already cached by
    /// fastembed) and loaded **synchronously on the calling thread** before
    /// this returns; the first construction can block for minutes. Sessions are
    /// shared process-wide and released once the last embedder using them
    /// drops, so repeated constructions only pay for what is not already live.
    pub fn try_with_options(opts: FastEmbedderOptions) -> Result<Self, QqlError> {
        // Validate the full BM25 text configuration before any model work.
        let bm25_text = Bm25TextConfig::resolve(
            opts.bm25_k1,
            opts.bm25_b,
            opts.bm25_avg_len,
            opts.bm25_language.as_deref(),
            opts.bm25_tokenizer.as_deref(),
            opts.bm25_lowercase,
            opts.bm25_ascii_folding,
            opts.bm25_stopwords.clone(),
            opts.bm25_stemmer.as_deref(),
            opts.bm25_min_token_len,
            opts.bm25_max_token_len,
            opts.bm25_stopwords_languages.clone(),
        )?;
        let bm25_params = bm25_text.params;

        let dense_model = match opts.model.as_deref() {
            None | Some("") => EmbeddingModel::default(),
            Some(name) => resolve_embedding_model(name)?,
        };
        let mut dense_init = InitOptionsWithLength::new(dense_model.clone());
        if let Some(ref dir) = opts.cache_dir {
            dense_init = dense_init.with_cache_dir(dir.clone());
        }
        dense_init = dense_init.with_show_download_progress(opts.show_download_progress);

        let dense_name = format!("{:?}", dense_model);
        let dense_info = TextEmbedding::get_model_info(&dense_model).map_err(|e| {
            err(format!(
                "fastembed has no model info for '{dense_name}': {e}"
            ))
        })?;
        let dense_dim = dense_info.dim;
        let dense_code = dense_info.model_code.clone();

        let cache_key = cache_dir_key(opts.cache_dir.as_ref());
        let dense_model_arc = load_cached(
            "dense",
            dense_cache(),
            (dense_name.clone(), cache_key.clone()),
            || {
                TextEmbedding::try_new(dense_init)
                    .map_err(|e| err(format!("fastembed dense init failed: {e}")))
            },
        )?;

        let sparse = match opts.sparse_model.as_deref() {
            None | Some("") => None,
            Some(name) => {
                let sp = resolve_sparse_model(name)?;
                let info = SparseTextEmbedding::get_model_info(&sp);
                let sparse_name = format!("{:?}", sp);
                let sparse_code = info.model_code.clone();
                let mut sparse_init = SparseInitOptions::new(sp);
                if let Some(ref dir) = opts.cache_dir {
                    sparse_init = sparse_init.with_cache_dir(dir.clone());
                }
                sparse_init = sparse_init.with_show_download_progress(opts.show_download_progress);
                let sparse_arc = load_cached(
                    "sparse",
                    sparse_cache(),
                    (sparse_name.clone(), cache_key.clone()),
                    || {
                        SparseTextEmbedding::try_new(sparse_init).map_err(|e| {
                            err(format!("fastembed SparseTextEmbedding init failed: {e}"))
                        })
                    },
                )?;
                Some(SparseSlot {
                    model: sparse_arc,
                    model_name: sparse_name,
                    model_code: sparse_code,
                })
            }
        };

        let multi = match opts.multi_model.as_deref() {
            None | Some("") => None,
            Some(name) => {
                let bge = resolve_multi_model(name)?;
                let multi_info = Bgem3Embedding::get_model_info(&bge);
                let multi_name = format!("{:?}", bge);
                let multi_code = multi_info.model_code.clone();
                // BGE-M3 ColBERT token dim matches dense output dim (1024).
                let multi_dim = multi_info.dim;

                let mut multi_init = Bgem3InitOptions::new(bge);
                if let Some(ref dir) = opts.cache_dir {
                    multi_init = multi_init.with_cache_dir(dir.clone());
                }
                multi_init = multi_init.with_show_download_progress(opts.show_download_progress);
                let multi_arc = load_cached(
                    "multi",
                    multi_cache(),
                    (multi_name.clone(), cache_key.clone()),
                    || {
                        Bgem3Embedding::try_new(multi_init)
                            .map_err(|e| err(format!("fastembed multi (BGE-M3) init failed: {e}")))
                    },
                )?;
                Some(MultiSlot {
                    model: multi_arc,
                    model_name: multi_name,
                    model_code: multi_code,
                    dim: multi_dim,
                })
            }
        };

        let image = match opts.image_model.as_deref() {
            None | Some("") => None,
            Some(name) => {
                let img = resolve_image_model(name)?;
                let info = ImageEmbedding::get_model_info(&img);
                let image_name = format!("{:?}", img);
                let image_code = info.model_code.clone();
                let image_dim = info.dim;

                let mut image_init = ImageInitOptions::new(img);
                if let Some(ref dir) = opts.cache_dir {
                    image_init = image_init.with_cache_dir(dir.clone());
                }
                image_init = image_init.with_show_download_progress(opts.show_download_progress);
                let image_arc = load_cached(
                    "image",
                    image_cache(),
                    (image_name.clone(), cache_key.clone()),
                    || {
                        ImageEmbedding::try_new(image_init).map_err(|e| {
                            err(format!("fastembed image (CLIP vision) init failed: {e}"))
                        })
                    },
                )?;
                Some(ImageSlot {
                    model: image_arc,
                    model_name: image_name,
                    model_code: image_code,
                    dim: image_dim,
                })
            }
        };

        let reranker = match opts.reranker_model.as_deref() {
            None | Some("") => None,
            Some(name) => {
                let rm = resolve_reranker_model(name)?;
                let info = TextRerank::get_model_info(&rm);
                let rerank_name = format!("{:?}", rm);
                let rerank_code = info.model_code.clone();
                let mut init = RerankInitOptions::new(rm);
                if let Some(ref dir) = opts.cache_dir {
                    init = init.with_cache_dir(dir.clone());
                }
                init = init.with_show_download_progress(opts.show_download_progress);
                let arc = load_cached(
                    "rerank",
                    rerank_cache(),
                    (rerank_name.clone(), cache_key.clone()),
                    || {
                        TextRerank::try_new(init)
                            .map_err(|e| err(format!("fastembed TextRerank init failed: {e}")))
                    },
                )?;
                Some(RerankSlot {
                    model: arc,
                    model_name: rerank_name,
                    model_code: rerank_code,
                })
            }
        };

        Ok(Self {
            dense: DenseSlot {
                model: dense_model_arc,
                model_name: dense_name,
                model_code: dense_code,
                dim: dense_dim,
            },
            sparse,
            multi,
            image,
            reranker,
            // The fallback encoder is Qdrant's own pipeline: text knobs
            // forward one-to-one (explicit stemmer/stopword overrides stay
            // at the language defaults — see `edge_bm25_config`).
            bm25: EdgeBm25::new(edge_bm25_config(&bm25_text)?)
                .map_err(|e| err(format!("edge BM25 init failed: {e}")))?,
            bm25_params,
            bm25_text,
        })
    }
}
