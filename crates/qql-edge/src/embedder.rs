use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fastembed::{
    Bgem3Embedding, Bgem3InitOptions, Bgem3Model, EmbeddingModel, ImageEmbedding,
    ImageEmbeddingModel, ImageInitOptions, InitOptionsWithLength, RerankInitOptions,
    RerankerModel, TextEmbedding, TextRerank,
};

use qql_core::error::QqlError;
use qql_embed::{Embedder, SparseVector};

fn err(msg: impl Into<std::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::execution("QQL-EDGE-EMBED", msg, None)
}

/// Public description of a local ONNX embedding model.
#[derive(Debug, Clone)]
pub struct EmbeddingModelInfo {
    /// Stable enum-style name, e.g. `"BGESmallENV15"`.
    pub name: String,
    /// HuggingFace / Xenova model code, e.g. `"Xenova/bge-small-en-v1.5"`.
    pub model_code: String,
    /// Output dimension of dense vectors.
    pub dim: usize,
    /// Short human description from fastembed.
    pub description: String,
    /// True when this entry is a multivector / ColBERT-capable offline model.
    pub multi: bool,
    /// True when this entry is an image / CLIP vision model.
    pub image: bool,
}

/// Options for constructing a [`FastEmbedder`].
#[derive(Debug, Clone, Default)]
pub struct FastEmbedderOptions {
    /// Dense model name. Accepts enum names (`BGESmallENV15`), HF codes
    /// (`Xenova/bge-small-en-v1.5`), or short aliases (`bge-small-en-v1.5`).
    /// For CLIP text use `ClipVitB32` / `Qdrant/clip-ViT-B-32-text`.
    /// `None` → default `BGESmallENV15` (384-d).
    pub model: Option<String>,
    /// Offline multivector model. Accepts `bge-m3`, `BGEM3Q`,
    /// `gpahal/bge-m3-onnx-int8`. When set, `embed_multi` runs via BGE-M3 ColBERT.
    pub multi_model: Option<String>,
    /// Offline image / CLIP vision model. Accepts `ClipVitB32`,
    /// `Qdrant/clip-ViT-B-32-vision`, `clip-vision`. Pairs with dense CLIP text.
    pub image_model: Option<String>,
    /// Offline cross-encoder reranker (`bge-reranker-base`, `BGERerankerBase`, …).
    pub reranker_model: Option<String>,
    /// Override model cache directory. `None` → fastembed default
    /// (`FASTEMBED_CACHE_DIR` / `HF_HOME` / `./.fastembed_cache`).
    pub cache_dir: Option<PathBuf>,
    /// Show HuggingFace download progress (default: `false` for bindings —
    /// progress bars on a Node/Python stderr are noise).
    pub show_download_progress: bool,
}

struct DenseSlot {
    model: Arc<Mutex<TextEmbedding>>,
    model_name: String,
    model_code: String,
    dim: usize,
}

struct MultiSlot {
    model: Arc<Mutex<Bgem3Embedding>>,
    model_name: String,
    model_code: String,
    /// Per-token ColBERT dimension (1024 for BGE-M3).
    dim: usize,
}

struct ImageSlot {
    model: Arc<Mutex<ImageEmbedding>>,
    model_name: String,
    model_code: String,
    dim: usize,
}

struct RerankSlot {
    model: Arc<Mutex<TextRerank>>,
    model_name: String,
    model_code: String,
}

pub struct FastEmbedder {
    dense: DenseSlot,
    multi: Option<MultiSlot>,
    image: Option<ImageSlot>,
    reranker: Option<RerankSlot>,
}

static DENSE_CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<TextEmbedding>>>>> = OnceLock::new();
static MULTI_CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<Bgem3Embedding>>>>> = OnceLock::new();
static IMAGE_CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<ImageEmbedding>>>>> = OnceLock::new();
static RERANK_CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<TextRerank>>>>> = OnceLock::new();

fn dense_cache() -> &'static Mutex<HashMap<String, Arc<Mutex<TextEmbedding>>>> {
    DENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn multi_cache() -> &'static Mutex<HashMap<String, Arc<Mutex<Bgem3Embedding>>>> {
    MULTI_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn image_cache() -> &'static Mutex<HashMap<String, Arc<Mutex<ImageEmbedding>>>> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rerank_cache() -> &'static Mutex<HashMap<String, Arc<Mutex<TextRerank>>>> {
    RERANK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

impl FastEmbedder {
    pub fn try_new(options: InitOptionsWithLength<EmbeddingModel>) -> Result<Self, QqlError> {
        Self::try_with_options(FastEmbedderOptions {
            model: Some(format!("{:?}", options.model_name)),
            multi_model: None,
            image_model: None,
            reranker_model: None,
            cache_dir: None,
            show_download_progress: false,
        })
    }

    /// Construct from high-level options (dense + optional multi, cache, progress).
    pub fn try_with_options(opts: FastEmbedderOptions) -> Result<Self, QqlError> {
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

        let dense_handle = dense_cache()
            .lock()
            .map_err(|e| err(format!("fastembed model cache poisoned: {e}")))?
            .get(&dense_name)
            .cloned();
        let dense_model_arc = if let Some(model) = dense_handle {
            model
        } else {
            let model = Arc::new(Mutex::new(
                TextEmbedding::try_new(dense_init)
                    .map_err(|e| err(format!("fastembed dense init failed: {e}")))?,
            ));
            let mut cache = dense_cache()
                .lock()
                .map_err(|e| err(format!("fastembed model cache poisoned: {e}")))?;
            Arc::clone(
                cache
                    .entry(dense_name.clone())
                    .or_insert_with(|| Arc::clone(&model)),
            )
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

                let multi_handle = multi_cache()
                    .lock()
                    .map_err(|e| err(format!("fastembed multi cache poisoned: {e}")))?
                    .get(&multi_name)
                    .cloned();
                let multi_arc = if let Some(model) = multi_handle {
                    model
                } else {
                    let mut multi_init = Bgem3InitOptions::new(bge);
                    if let Some(ref dir) = opts.cache_dir {
                        multi_init = multi_init.with_cache_dir(dir.clone());
                    }
                    multi_init =
                        multi_init.with_show_download_progress(opts.show_download_progress);
                    let model = Arc::new(Mutex::new(
                        Bgem3Embedding::try_new(multi_init)
                            .map_err(|e| err(format!("fastembed multi (BGE-M3) init failed: {e}")))?,
                    ));
                    let mut cache = multi_cache()
                        .lock()
                        .map_err(|e| err(format!("fastembed multi cache poisoned: {e}")))?;
                    Arc::clone(
                        cache
                            .entry(multi_name.clone())
                            .or_insert_with(|| Arc::clone(&model)),
                    )
                };
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

                let image_handle = image_cache()
                    .lock()
                    .map_err(|e| err(format!("fastembed image cache poisoned: {e}")))?
                    .get(&image_name)
                    .cloned();
                let image_arc = if let Some(model) = image_handle {
                    model
                } else {
                    let mut image_init = ImageInitOptions::new(img);
                    if let Some(ref dir) = opts.cache_dir {
                        image_init = image_init.with_cache_dir(dir.clone());
                    }
                    image_init =
                        image_init.with_show_download_progress(opts.show_download_progress);
                    let model = Arc::new(Mutex::new(
                        ImageEmbedding::try_new(image_init).map_err(|e| {
                            err(format!("fastembed image (CLIP vision) init failed: {e}"))
                        })?,
                    ));
                    let mut cache = image_cache()
                        .lock()
                        .map_err(|e| err(format!("fastembed image cache poisoned: {e}")))?;
                    Arc::clone(
                        cache
                            .entry(image_name.clone())
                            .or_insert_with(|| Arc::clone(&model)),
                    )
                };
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
                let handle = rerank_cache()
                    .lock()
                    .map_err(|e| err(format!("fastembed rerank cache poisoned: {e}")))?
                    .get(&rerank_name)
                    .cloned();
                let arc = if let Some(model) = handle {
                    model
                } else {
                    let mut init = RerankInitOptions::new(rm);
                    if let Some(ref dir) = opts.cache_dir {
                        init = init.with_cache_dir(dir.clone());
                    }
                    init = init.with_show_download_progress(opts.show_download_progress);
                    let model = Arc::new(Mutex::new(
                        TextRerank::try_new(init)
                            .map_err(|e| err(format!("fastembed TextRerank init failed: {e}")))?,
                    ));
                    let mut cache = rerank_cache()
                        .lock()
                        .map_err(|e| err(format!("fastembed rerank cache poisoned: {e}")))?;
                    Arc::clone(
                        cache
                            .entry(rerank_name.clone())
                            .or_insert_with(|| Arc::clone(&model)),
                    )
                };
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
            multi,
            image,
            reranker,
        })
    }

    pub fn try_default() -> Result<Self, QqlError> {
        Self::try_with_options(FastEmbedderOptions::default())
    }

    /// Build from a dense model name string. See [`resolve_embedding_model`].
    pub fn try_from_model(model: &str) -> Result<Self, QqlError> {
        Self::try_with_options(FastEmbedderOptions {
            model: Some(model.to_string()),
            ..Default::default()
        })
    }

    pub fn model_name(&self) -> &str {
        &self.dense.model_name
    }

    pub fn model_code(&self) -> &str {
        &self.dense.model_code
    }

    pub fn dimension(&self) -> usize {
        self.dense.dim
    }

    pub fn multi_model_name(&self) -> Option<&str> {
        self.multi.as_ref().map(|m| m.model_name.as_str())
    }

    pub fn multi_model_code(&self) -> Option<&str> {
        self.multi.as_ref().map(|m| m.model_code.as_str())
    }

    pub fn multi_dimension(&self) -> Option<usize> {
        self.multi.as_ref().map(|m| m.dim)
    }

    pub fn has_multi(&self) -> bool {
        self.multi.is_some()
    }

    pub fn image_model_name(&self) -> Option<&str> {
        self.image.as_ref().map(|m| m.model_name.as_str())
    }

    pub fn image_model_code(&self) -> Option<&str> {
        self.image.as_ref().map(|m| m.model_code.as_str())
    }

    pub fn image_dimension(&self) -> Option<usize> {
        self.image.as_ref().map(|m| m.dim)
    }

    pub fn has_image(&self) -> bool {
        self.image.is_some()
    }

    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some()
    }

    pub fn reranker_model_code(&self) -> Option<&str> {
        self.reranker.as_ref().map(|m| m.model_code.as_str())
    }

    fn accepts_reranker_model(&self, requested: &str) -> bool {
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
        if let Some(ref multi) = self.multi {
            if r.eq_ignore_ascii_case(&multi.model_name)
                || r.eq_ignore_ascii_case(&multi.model_code)
                || short_alias_matches(r, &multi.model_code)
                || is_multi_alias(r)
            {
                return true;
            }
        }
        if let Some(ref image) = self.image {
            if r.eq_ignore_ascii_case(&image.model_name)
                || r.eq_ignore_ascii_case(&image.model_code)
                || short_alias_matches(r, &image.model_code)
                || is_image_alias(r)
            {
                return true;
            }
        }
        false
    }

    fn accepts_image_model(&self, requested: &str) -> bool {
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

    fn accepts_dense_model(&self, requested: &str) -> bool {
        let r = requested.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("default") {
            return true;
        }
        r.eq_ignore_ascii_case(&self.dense.model_name)
            || r.eq_ignore_ascii_case(&self.dense.model_code)
            || short_alias_matches(r, &self.dense.model_code)
    }

    fn accepts_multi_model(&self, requested: &str) -> bool {
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

/// List every dense text model fastembed can load, plus offline multi (BGE-M3).
pub fn list_embedding_models() -> Vec<EmbeddingModelInfo> {
    let mut models: Vec<EmbeddingModelInfo> = TextEmbedding::list_supported_models()
        .into_iter()
        .map(|m| EmbeddingModelInfo {
            name: format!("{:?}", m.model),
            model_code: m.model_code,
            dim: m.dim,
            description: m.description,
            multi: false,
            image: false,
        })
        .collect();
    for m in Bgem3Embedding::list_supported_models() {
        models.push(EmbeddingModelInfo {
            name: format!("{:?}", m.model),
            model_code: m.model_code,
            dim: m.dim,
            description: format!("{} (multivector / ColBERT via BGE-M3)", m.description),
            multi: true,
            image: false,
        });
    }
    for m in ImageEmbedding::list_supported_models() {
        models.push(EmbeddingModelInfo {
            name: format!("{:?}", m.model),
            model_code: m.model_code,
            dim: m.dim,
            description: format!("{} (image / CLIP vision)", m.description),
            multi: false,
            image: true,
        });
    }
    models
}

/// Resolve a user-facing dense model string to an [`EmbeddingModel`].
///
/// Accepts, case-insensitively:
/// - enum Debug names: `BGESmallENV15`, `AllMiniLML6V2`
/// - HuggingFace model codes: `Xenova/bge-small-en-v1.5`
/// - short aliases: `bge-small-en-v1.5`, `all-minilm-l6-v2`
pub fn resolve_embedding_model(name: &str) -> Result<EmbeddingModel, QqlError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(EmbeddingModel::default());
    }

    // 1. Debug / FromStr name
    if let Ok(m) = name.parse::<EmbeddingModel>() {
        return Ok(m);
    }

    // 2. Exact model_code / short slug (case-insensitive)
    for info in TextEmbedding::list_supported_models() {
        if info.model_code.eq_ignore_ascii_case(name) {
            return Ok(info.model);
        }
        if short_alias_matches(name, &info.model_code) {
            return Ok(info.model);
        }
        if let Some(slug) = info.model_code.rsplit('/').next() {
            if slug.eq_ignore_ascii_case(name) {
                return Ok(info.model);
            }
        }
    }

    // Suggest a few options so callers aren't left guessing
    let mut suggestions: Vec<String> = TextEmbedding::list_supported_models()
        .into_iter()
        .map(|m| format!("{:?} ({}, {}-d)", m.model, m.model_code, m.dim))
        .take(6)
        .collect();
    suggestions.sort();
    Err(err(format!(
        "unknown embedding model '{name}'. Use list_embedding_models() for the full list. Examples: {}",
        suggestions.join("; ")
    )))
}

/// Resolve offline multi model (`bge-m3`, HF code, etc.).
pub fn resolve_multi_model(name: &str) -> Result<Bgem3Model, QqlError> {
    let name = name.trim();
    if name.is_empty() || is_multi_alias(name) {
        return Ok(Bgem3Model::default());
    }
    if let Ok(m) = name.parse::<Bgem3Model>() {
        return Ok(m);
    }
    for info in Bgem3Embedding::list_supported_models() {
        if info.model_code.eq_ignore_ascii_case(name)
            || short_alias_matches(name, &info.model_code)
            || format!("{:?}", info.model).eq_ignore_ascii_case(name)
        {
            return Ok(info.model);
        }
    }
    Err(err(format!(
        "unknown multi embedding model '{name}'. Offline multi uses BGE-M3 \
         (e.g. 'bge-m3', 'BGEM3Q', 'gpahal/bge-m3-onnx-int8')"
    )))
}

fn is_multi_alias(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "bge-m3"
            | "bgem3"
            | "bgem3q"
            | "colbert"
            | "multi"
            | "multivector"
            | "late-interaction"
    )
}

/// Resolve offline image model (CLIP vision, etc.).
pub fn resolve_image_model(name: &str) -> Result<ImageEmbeddingModel, QqlError> {
    let name = name.trim();
    if name.is_empty() || is_image_alias(name) {
        return Ok(ImageEmbeddingModel::default());
    }
    if let Ok(m) = name.parse::<ImageEmbeddingModel>() {
        return Ok(m);
    }
    for info in ImageEmbedding::list_supported_models() {
        if info.model_code.eq_ignore_ascii_case(name)
            || short_alias_matches(name, &info.model_code)
            || format!("{:?}", info.model).eq_ignore_ascii_case(name)
        {
            return Ok(info.model);
        }
    }
    Err(err(format!(
        "unknown image embedding model '{name}'. Offline image uses CLIP vision \
         (e.g. 'ClipVitB32', 'Qdrant/clip-ViT-B-32-vision', 'clip-vision')"
    )))
}

fn is_image_alias(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "clip"
            | "clip-vision"
            | "clip_vision"
            | "clip-vit-b-32"
            | "clip-vit-b-32-vision"
            | "image"
            | "vision"
    )
}

/// Resolve offline cross-encoder model id.
pub fn resolve_reranker_model(name: &str) -> Result<RerankerModel, QqlError> {
    let name = name.trim();
    if name.is_empty() || is_reranker_alias(name) {
        return Ok(RerankerModel::default());
    }
    if let Ok(m) = name.parse::<RerankerModel>() {
        return Ok(m);
    }
    for info in TextRerank::list_supported_models() {
        if info.model_code.eq_ignore_ascii_case(name)
            || short_alias_matches(name, &info.model_code)
            || format!("{:?}", info.model).eq_ignore_ascii_case(name)
        {
            return Ok(info.model);
        }
    }
    Err(err(format!(
        "unknown reranker model '{name}'. Examples: bge-reranker-base, BGERerankerBase, \
         BAAI/bge-reranker-base, jina-reranker-v1-turbo-en"
    )))
}

fn is_reranker_alias(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "rerank"
            | "reranker"
            | "cross-encoder"
            | "cross_encoder"
            | "bge-reranker"
            | "bge-reranker-base"
    )
}

fn short_alias_matches(requested: &str, model_code: &str) -> bool {
    let req = requested.trim().trim_matches('"');
    let code = model_code;
    if req.eq_ignore_ascii_case(code) {
        return true;
    }
    // strip org prefix: "Xenova/bge-small-en-v1.5" ↔ "bge-small-en-v1.5"
    if let Some(slug) = code.rsplit('/').next() {
        if req.eq_ignore_ascii_case(slug) {
            return true;
        }
    }
    // Strip common suffixes people omit when referring to a converted model.
    ["-onnx-q", "-onnx", "-q4_k_m", "-q8_0", "-onnx-int8", "-int8"]
        .iter()
        .any(|suffix| {
            code.strip_suffix(suffix)
                .is_some_and(|base| req.eq_ignore_ascii_case(base))
                || code
                    .rsplit('/')
                    .next()
                    .and_then(|slug| slug.strip_suffix(suffix))
                    .is_some_and(|base| req.eq_ignore_ascii_case(base))
        })
}

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

    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError> {
        Ok(qql_embed::sparse::build_query_default(text))
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
        if !self.accepts_multi_model(model)
            && !(model.is_empty() || model.eq_ignore_ascii_case("default"))
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
        if !self.accepts_image_model(model)
            && !(model.is_empty() || model.eq_ignore_ascii_case("default"))
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
        if !self.accepts_image_model(model)
            && !(model.is_empty() || model.eq_ignore_ascii_case("default"))
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
        if !self.accepts_reranker_model(model)
            && !(model.is_empty() || model.eq_ignore_ascii_case("default"))
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
