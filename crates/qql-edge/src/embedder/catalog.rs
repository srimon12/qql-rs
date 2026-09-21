//! Model catalog: list and resolve fastembed models by name, code, or alias.
//!
//! Pure move from `embedder.rs` (size hygiene split).

use fastembed::{
    Bgem3Embedding, Bgem3Model, EmbeddingModel, ImageEmbedding, ImageEmbeddingModel, RerankerModel,
    SparseModel, SparseTextEmbedding, TextEmbedding, TextRerank,
};
use qql_core::error::QqlError;

use super::{EmbeddingModelInfo, err};

/// List dense text models, offline sparse models (SPLADE / BGE-M3),
/// multi (BGE-M3 / ColBERT), and image models (CLIP vision, …) that
/// fastembed can load.
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
    for m in SparseTextEmbedding::list_supported_models() {
        models.push(EmbeddingModelInfo {
            name: format!("{:?}", m.model),
            model_code: m.model_code,
            dim: m.dim,
            description: format!("{} (sparse / SPLADE)", m.description),
            multi: false,
            image: false,
        });
    }
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
        if let Some(slug) = info.model_code.rsplit('/').next()
            && slug.eq_ignore_ascii_case(name)
        {
            return Ok(info.model);
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

pub(crate) fn is_multi_alias(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "bge-m3" | "bgem3" | "bgem3q" | "colbert" | "multi" | "multivector" | "late-interaction"
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

pub(crate) fn is_image_alias(name: &str) -> bool {
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
pub(crate) fn resolve_reranker_model(name: &str) -> Result<RerankerModel, QqlError> {
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

pub(crate) fn is_reranker_alias(name: &str) -> bool {
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

/// Resolve offline sparse model (SPLADE, BGE-M3 sparse).
pub(crate) fn resolve_sparse_model(name: &str) -> Result<SparseModel, QqlError> {
    let name = name.trim();
    if name.is_empty()
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "splade" | "spladeppv1" | "splade_pp_en_v1" | "sparse" | "bm25"
        )
    {
        return Ok(SparseModel::default());
    }
    if matches!(name.to_ascii_lowercase().as_str(), "bge-m3" | "bgem3") {
        return Ok(SparseModel::BGEM3);
    }
    if let Ok(m) = name.parse::<SparseModel>() {
        return Ok(m);
    }
    for info in SparseTextEmbedding::list_supported_models() {
        if info.model_code.eq_ignore_ascii_case(name)
            || short_alias_matches(name, &info.model_code)
            || format!("{:?}", info.model).eq_ignore_ascii_case(name)
        {
            return Ok(info.model);
        }
    }
    Err(err(format!(
        "unknown sparse embedding model '{name}'. Offline sparse supports \
         'splade' (SPLADEPPV1) and 'bge-m3' (BGEM3)"
    )))
}

pub(crate) fn is_sparse_alias(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "splade" | "spladeppv1" | "splade_pp_en_v1" | "sparse" | "bm25" | "bge-m3" | "bgem3"
    )
}

pub(crate) fn short_alias_matches(requested: &str, model_code: &str) -> bool {
    let req = requested.trim().trim_matches('"');
    let code = model_code;
    if req.eq_ignore_ascii_case(code) {
        return true;
    }
    // strip org prefix: "Xenova/bge-small-en-v1.5" ↔ "bge-small-en-v1.5"
    if let Some(slug) = code.rsplit('/').next()
        && req.eq_ignore_ascii_case(slug)
    {
        return true;
    }
    // Strip common suffixes people omit when referring to a converted model.
    [
        "-onnx-q",
        "-onnx",
        "-q4_k_m",
        "-q8_0",
        "-onnx-int8",
        "-int8",
    ]
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
