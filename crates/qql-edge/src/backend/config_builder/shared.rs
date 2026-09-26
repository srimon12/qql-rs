//! Shared config primitives: placement mapping, memory decode, error constructor.
//!
//! Pure move from `config_builder.rs` (size hygiene split).

use qql_core::ast::MemoryPlacement;
use qql_core::error::QqlError;

/// Resolve the engine's storage placement from the plan's legacy `on_disk`
/// flag and the 1.19 `memory` tier.
///
/// qdrant-edge 0.8 exposes no `Memory` enum on vector/sparse params: the only
/// storage switch is RAM (`on_disk = false`) vs mmap (`on_disk = true`). The
/// mapping is therefore lossy in one direction, and deliberately so:
///
/// | plan `memory` | edge `on_disk` | why |
/// |---------------|----------------|-----|
/// | `pinned`      | `false`        | kept in RAM, never evicted |
/// | `cached`      | `true`         | disk-backed; the OS page cache approximates preloading |
/// | `cold`        | `true`         | disk-backed, read on demand |
///
/// `memory` wins over the deprecated `on_disk` when both are set, mirroring
/// Qdrant 1.19. `None` leaves the engine default (`false`, i.e. RAM).
pub(crate) fn resolve_on_disk(
    on_disk: Option<bool>,
    memory: Option<MemoryPlacement>,
) -> Option<bool> {
    match memory {
        Some(MemoryPlacement::Pinned) => Some(false),
        Some(MemoryPlacement::Cached | MemoryPlacement::Cold) => Some(true),
        None => on_disk,
    }
}

/// Decode a placement keyword into an engine `Memory` value.
///
/// qdrant-edge does not re-export `Memory`; the field type is inferred from the
/// assignment site (`HnswIndexConfig::memory`, quantization configs). The
/// keyword is fed through a serde string deserializer — no `serde_json::Value`
/// is built for what is a plain enum decode.
pub(crate) fn edge_memory<T: serde::de::DeserializeOwned>(
    placement: MemoryPlacement,
) -> Result<T, QqlError> {
    let keyword = placement.as_str();
    let deserializer = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(keyword);
    T::deserialize(deserializer).map_err(|error| {
        edge_config_error(format!("invalid memory placement '{keyword}': {error}"))
    })
}

pub(crate) fn usize_config(field: &str, value: u64) -> Result<usize, QqlError> {
    usize::try_from(value)
        .map_err(|error| edge_config_error(format!("{field} is too large: {error}")))
}

pub(crate) fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}
