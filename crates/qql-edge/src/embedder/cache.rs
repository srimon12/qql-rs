//! Process-global model caches and the race-free loader.
//!
//! Entries hold [`Weak`] references: the configured [`super::FastEmbedder`] is
//! the only strong holder, so dropping the last embedder using a model releases
//! its ONNX session (a later construction reloads it) instead of pinning it in
//! RAM for the process lifetime. [`load_cached`] serializes concurrent
//! constructions of the same model, so two hosts racing on one model load it
//! once rather than transiently holding two copies.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use fastembed::{Bgem3Embedding, ImageEmbedding, SparseTextEmbedding, TextEmbedding, TextRerank};
use qql_core::error::QqlError;

use super::err;

/// `(model name, cache dir)` — the identity of a cached model.
pub(crate) type CacheKey = (String, String);
/// A loaded model, shared by every slot that uses it.
pub(crate) type CachedModel<T> = Arc<Mutex<T>>;
/// Loaded-model cache; values are weak so the last slot's drop frees the model.
pub(crate) type ModelCache<T> = Mutex<HashMap<CacheKey, Weak<Mutex<T>>>>;

static DENSE_CACHE: OnceLock<ModelCache<TextEmbedding>> = OnceLock::new();
static SPARSE_CACHE: OnceLock<ModelCache<SparseTextEmbedding>> = OnceLock::new();
static MULTI_CACHE: OnceLock<ModelCache<Bgem3Embedding>> = OnceLock::new();
static IMAGE_CACHE: OnceLock<ModelCache<ImageEmbedding>> = OnceLock::new();
static RERANK_CACHE: OnceLock<ModelCache<TextRerank>> = OnceLock::new();

/// Per-`(slot, model)` load mutexes. Entries are never removed: the key space
/// is the fastembed catalog (unknown names fail before the cache is consulted),
/// so growth is bounded by the models a process actually configures.
type LoadLocks = Mutex<HashMap<(&'static str, CacheKey), Arc<Mutex<()>>>>;
static LOAD_LOCKS: OnceLock<LoadLocks> = OnceLock::new();

pub(crate) fn cache_dir_key(dir: Option<&PathBuf>) -> String {
    dir.map(|p| p.display().to_string()).unwrap_or_default()
}

pub(crate) fn dense_cache() -> &'static ModelCache<TextEmbedding> {
    DENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn sparse_cache() -> &'static ModelCache<SparseTextEmbedding> {
    SPARSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn multi_cache() -> &'static ModelCache<Bgem3Embedding> {
    MULTI_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn image_cache() -> &'static ModelCache<ImageEmbedding> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn rerank_cache() -> &'static ModelCache<TextRerank> {
    RERANK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load `key` through `cache`, calling `load` at most once per live model.
///
/// Double-checked: a live weak entry is upgraded without locking the load
/// mutex; otherwise the per-key load mutex serializes the (potentially
/// multi-minute) download/parse so racing constructions reuse one model.
pub(crate) fn load_cached<T>(
    slot: &'static str,
    cache: &ModelCache<T>,
    key: CacheKey,
    load: impl FnOnce() -> Result<T, QqlError>,
) -> Result<CachedModel<T>, QqlError> {
    if let Some(model) = cached(cache, &key)? {
        return Ok(model);
    }

    let lock = {
        let mut locks = load_locks()
            .lock()
            .map_err(|error| err(format!("model load-lock map poisoned: {error}")))?;
        Arc::clone(
            locks
                .entry((slot, key.clone()))
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    };
    let _loading = lock
        .lock()
        .map_err(|error| err(format!("model load lock poisoned: {error}")))?;

    if let Some(model) = cached(cache, &key)? {
        return Ok(model);
    }

    let model = Arc::new(Mutex::new(load()?));
    cache
        .lock()
        .map_err(|error| err(format!("model cache poisoned: {error}")))?
        .insert(key, Arc::downgrade(&model));
    Ok(model)
}

fn cached<T>(cache: &ModelCache<T>, key: &CacheKey) -> Result<Option<CachedModel<T>>, QqlError> {
    let cache = cache
        .lock()
        .map_err(|error| err(format!("model cache poisoned: {error}")))?;
    Ok(cache.get(key).and_then(Weak::upgrade))
}

fn load_locks() -> &'static LoadLocks {
    LOAD_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}
