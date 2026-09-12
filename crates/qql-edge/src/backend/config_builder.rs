//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.

use qdrant_edge::{
    BinaryQuantizationConfig, BinaryQuantizationEncoding, BinaryQuantizationQueryEncoding,
    CompressionRatio, EdgeConfigBuilder, ProductQuantizationConfig, QuantizationConfig,
    ScalarQuantizationConfig, ScalarType,
};

use qql_core::ast::{MemoryPlacement, VectorDatatype, VectorDistance};
use qql_core::error::QqlError;
use qql_plan::{
    CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig, HnswConfig, OptimizersConfig,
    SparseVectorParams,
};

use super::unsupported::EdgeUnsupported;

pub(crate) fn build_edge_config(
    req: &CreateCollectionRequest,
    on_disk_payload: bool,
) -> Result<qdrant_edge::EdgeConfig, QqlError> {
    if req.wal_config.is_some() {
        return Err(EdgeUnsupported::Wal.error());
    }
    if req.strict_mode_config.is_some() {
        return Err(EdgeUnsupported::StrictMode.error());
    }
    if req.metadata.is_some() {
        return Err(EdgeUnsupported::Metadata.error());
    }
    // A create-time `WITH PARAMS (on_disk_payload = …)` overrides the
    // executor-level default for this collection; every other param key is
    // rejected before we get here (see `unsupported::reject_collection_params`).
    let on_disk_payload = req
        .params
        .as_ref()
        .and_then(|params| params.on_disk_payload)
        .unwrap_or(on_disk_payload);
    // Resolve the collection-wide HNSW config once. A per-vector `hnsw_config`
    // is a full replacement for the engine (`hnsw_config.unwrap_or(global)`),
    // so per-vector overrides are field-merged over this resolved base to keep
    // the server's field-wise diff semantics.
    let global_hnsw = req.hnsw_config.as_ref().map(edge_hnsw_config).transpose()?;
    let mut builder = EdgeConfigBuilder::new().on_disk_payload(on_disk_payload);

    match &req.vectors {
        Some(DenseVectorsConfig::Single(params)) => {
            builder = builder.vector(
                String::new(),
                edge_vector_params("<default>", params, global_hnsw.as_ref())?,
            );
        }
        Some(DenseVectorsConfig::Named(map)) => {
            for (name, params) in map {
                builder = builder.vector(
                    name.clone(),
                    edge_vector_params(name, params, global_hnsw.as_ref())?,
                );
            }
        }
        None => {}
    }

    if let Some(ref sparse) = req.sparse_vectors {
        for (name, params) in sparse {
            builder = builder.sparse_vector(name.clone(), edge_sparse_vector_params(name, params)?);
        }
    }

    if let Some(global) = global_hnsw {
        builder = builder.hnsw_config(global);
    }

    if let Some(ref qc) = req.quantization_config {
        builder = builder.quantization_config(edge_quantization_config(qc)?);
    }

    if let Some(ref oc) = req.optimizers_config {
        builder = builder.optimizers(edge_optimizers_config(oc)?);
    }

    Ok(builder.build())
}

/// Lower a plan HNSW config into the qdrant-edge segment config type.
///
/// Shared by `CREATE COLLECTION` and the `ALTER COLLECTION` HNSW setter so the
/// two cannot drift. The engine's `HnswConfig` has required fields (`m`,
/// `ef_construct`, `full_scan_threshold`) while QQL allows any subset, so the
/// provided keys are overlaid onto the engine's own defaults — a plain serde
/// parse would reject a partial config.
pub(crate) fn edge_hnsw_config(
    config: &HnswConfig,
) -> Result<qdrant_edge::HnswIndexConfig, QqlError> {
    edge_hnsw_config_over(config, &qdrant_edge::HnswIndexConfig::default())
}

/// Overlay a plan optimizer patch onto an existing engine config.
///
/// `set_optimizers_config` replaces the whole blob, so a partial
/// `ALTER … WITH OPTIMIZERS (indexing_threshold = …)` must keep the other
/// keys already persisted on the shard.
pub(crate) fn overlay_optimizers(
    plan: &OptimizersConfig,
    base: &qdrant_edge::EdgeOptimizersConfig,
) -> Result<qdrant_edge::EdgeOptimizersConfig, QqlError> {
    let patch = edge_optimizers_config(plan)?;
    Ok(qdrant_edge::EdgeOptimizersConfig {
        deleted_threshold: patch.deleted_threshold.or(base.deleted_threshold),
        vacuum_min_vector_number: patch
            .vacuum_min_vector_number
            .or(base.vacuum_min_vector_number),
        default_segment_number: patch.default_segment_number.or(base.default_segment_number),
        max_segment_size: patch.max_segment_size.or(base.max_segment_size),
        indexing_threshold: patch.indexing_threshold.or(base.indexing_threshold),
        prevent_unoptimized: patch.prevent_unoptimized.or(base.prevent_unoptimized),
    })
}

/// Field-merge a plan HNSW config over `base`.
///
/// Used for per-vector overrides, `ALTER COLLECTION` (live shard config as
/// base), and plain create-time configs (engine defaults as base). Engine
/// `Memory` is not re-exported, so that one field is decoded from its keyword
/// via serde (`StrDeserializer`), not a `serde_json::Value`.
#[allow(deprecated)]
pub(crate) fn edge_hnsw_config_over(
    config: &HnswConfig,
    base: &qdrant_edge::HnswIndexConfig,
) -> Result<qdrant_edge::HnswIndexConfig, QqlError> {
    let mut merged = *base;
    if let Some(m) = config.m {
        merged.m = usize_config("hnsw.m", m)?;
    }
    if let Some(ef_construct) = config.ef_construct {
        merged.ef_construct = usize_config("hnsw.ef_construct", ef_construct)?;
    }
    if let Some(full_scan_threshold) = config.full_scan_threshold {
        merged.full_scan_threshold = usize_config("hnsw.full_scan_threshold", full_scan_threshold)?;
    }
    if let Some(max_indexing_threads) = config.max_indexing_threads {
        merged.max_indexing_threads =
            usize_config("hnsw.max_indexing_threads", max_indexing_threads)?;
    }
    if let Some(on_disk) = config.on_disk {
        merged.on_disk = Some(on_disk);
    }
    if let Some(payload_m) = config.payload_m {
        merged.payload_m = Some(usize_config("hnsw.payload_m", payload_m)?);
    }
    if let Some(inline_storage) = config.inline_storage {
        merged.inline_storage = Some(inline_storage);
    }
    if let Some(memory) = config.memory {
        merged.memory = Some(edge_memory(memory)?);
    }
    Ok(merged)
}

/// Lower a plan optimizer config into the qdrant-edge optimizer config type.
///
/// Shared by `CREATE COLLECTION` and the `ALTER COLLECTION` optimizer setter so
/// the two cannot drift. qdrant-edge deliberately excludes `memmap_threshold`
/// (deprecated upstream), `flush_interval_sec` (edge does not flush on a
/// timer), and `max_optimization_threads` (optimizations are manual); those
/// keys fail closed instead of being silently dropped.
pub(crate) fn edge_optimizers_config(
    config: &OptimizersConfig,
) -> Result<qdrant_edge::EdgeOptimizersConfig, QqlError> {
    for (key, present) in [
        ("memmap_threshold", config.memmap_threshold.is_some()),
        ("flush_interval_sec", config.flush_interval_sec.is_some()),
        (
            "max_optimization_threads",
            config.max_optimization_threads.is_some(),
        ),
    ] {
        if present {
            return Err(EdgeUnsupported::OptimizerKey
                .error()
                .with_field("config_key", key));
        }
    }
    Ok(qdrant_edge::EdgeOptimizersConfig {
        deleted_threshold: config.deleted_threshold,
        vacuum_min_vector_number: config
            .vacuum_min_vector_number
            .map(|value| usize_config("optimizers.vacuum_min_vector_number", value))
            .transpose()?,
        default_segment_number: config
            .default_segment_number
            .map(|value| usize_config("optimizers.default_segment_number", value))
            .transpose()?,
        max_segment_size: config
            .max_segment_size
            .map(|value| usize_config("optimizers.max_segment_size", value))
            .transpose()?,
        indexing_threshold: config
            .indexing_threshold
            .map(|value| usize_config("optimizers.indexing_threshold", value))
            .transpose()?,
        prevent_unoptimized: config.prevent_unoptimized,
    })
}

fn edge_vector_params(
    name: &str,
    params: &DenseVectorParams,
    global_hnsw: Option<&qdrant_edge::HnswIndexConfig>,
) -> Result<qdrant_edge::EdgeVectorParams, QqlError> {
    let size = usize::try_from(params.size).map_err(|error| {
        edge_config_error(format!("vector '{name}' size is too large: {error}"))
    })?;
    let distance = match params.distance {
        VectorDistance::Cosine => qdrant_edge::Distance::Cosine,
        VectorDistance::Dot => qdrant_edge::Distance::Dot,
        VectorDistance::Euclid => qdrant_edge::Distance::Euclid,
        VectorDistance::Manhattan => qdrant_edge::Distance::Manhattan,
    };
    let multivector_config = params
        .multivector_config
        .map(|_| qdrant_edge::MultiVectorConfig {
            comparator: qdrant_edge::MultiVectorComparator::MaxSim,
        });
    let quantization_config = params
        .quantization_config
        .as_ref()
        .map(edge_quantization_config)
        .transpose()?;
    let hnsw_config = match params.hnsw_config.as_ref() {
        Some(per_vector) => Some(edge_hnsw_config_over(
            per_vector,
            global_hnsw.unwrap_or(&qdrant_edge::HnswIndexConfig::default()),
        )?),
        None => None,
    };
    Ok(qdrant_edge::EdgeVectorParams {
        size,
        distance,
        on_disk: resolve_on_disk(params.on_disk, params.memory),
        multivector_config,
        datatype: params.datatype.map(edge_datatype),
        quantization_config,
        hnsw_config,
    })
}

/// Lower a plan sparse vector config onto the engine type.
///
/// qdrant-edge always stores sparse vector *weights* as mmap
/// (`SparseVectorStorageType::Mmap`); the `on_disk` flag the engine accepts
/// only selects the sparse **index** placement used by the optimizer
/// (`MutableRam`/`ImmutableRam` vs `Mmap`). The plan's `memory` tier is mapped
/// onto that flag — see [`resolve_on_disk`] — and `datatype: turbo4` fails
/// closed because sparse indexes support float32/float16/uint8 only.
fn edge_sparse_vector_params(
    name: &str,
    params: &SparseVectorParams,
) -> Result<qdrant_edge::EdgeSparseVectorParams, QqlError> {
    let index = params.index.as_ref();
    let datatype = match index.and_then(|index| index.datatype) {
        Some(VectorDatatype::Turbo4) => {
            return Err(edge_config_error(format!(
                "sparse vector '{name}': datatype turbo4 is not supported for sparse vectors (expected float32, float16, or uint8)"
            )));
        }
        other => other.map(edge_datatype),
    };
    let full_scan_threshold = index
        .and_then(|index| index.full_scan_threshold)
        .map(|value| {
            usize::try_from(value).map_err(|error| {
                edge_config_error(format!(
                    "sparse vector '{name}' full_scan_threshold is too large: {error}"
                ))
            })
        })
        .transpose()?;
    Ok(qdrant_edge::EdgeSparseVectorParams {
        full_scan_threshold,
        on_disk: resolve_on_disk(
            index.and_then(|index| index.on_disk),
            index.and_then(|index| index.memory),
        ),
        modifier: Some(match params.modifier {
            qql_plan::SparseModifier::Idf => qdrant_edge::Modifier::Idf,
            qql_plan::SparseModifier::None => qdrant_edge::Modifier::None,
        }),
        datatype,
    })
}

/// Lower a plan quantization config onto the engine type.
///
/// Families map field-for-field. Engine `Memory` is not re-exported, so
/// placement is decoded from its keyword through `StrDeserializer` (no
/// `serde_json::Value`). Turbo is the one family that still goes through JSON:
/// `TurboQuantization`, `TurboQuantQuantizationConfig`, and
/// `TurboQuantBitSize` are not re-exported, and unlike the scalar/product/
/// binary configs there is no `From<TurboQuantization>` constructor, so the
/// nested object cannot be built by name.
#[allow(deprecated)]
fn edge_quantization_config(
    config: &qql_plan::QuantizationConfig,
) -> Result<QuantizationConfig, QqlError> {
    match config {
        qql_plan::QuantizationConfig::Scalar { scalar } => {
            if !scalar.qtype.eq_ignore_ascii_case("int8") {
                return Err(edge_config_error(format!(
                    "scalar quantization type '{}' is not supported (expected int8)",
                    scalar.qtype
                )));
            }
            Ok(QuantizationConfig::from(ScalarQuantizationConfig {
                r#type: ScalarType::Int8,
                quantile: scalar.quantile.map(|value| value as f32),
                always_ram: scalar.always_ram,
                memory: scalar.memory.map(edge_memory).transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Product { product } => {
            Ok(QuantizationConfig::from(ProductQuantizationConfig {
                compression: edge_compression(&product.compression)?,
                always_ram: product.always_ram,
                memory: product.memory.map(edge_memory).transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Binary { binary } => {
            Ok(QuantizationConfig::from(BinaryQuantizationConfig {
                always_ram: binary.always_ram,
                memory: binary.memory.map(edge_memory).transpose()?,
                encoding: binary
                    .encoding
                    .as_deref()
                    .map(edge_binary_encoding)
                    .transpose()?,
                query_encoding: binary
                    .query_encoding
                    .as_deref()
                    .map(edge_binary_query_encoding)
                    .transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Turbo { turbo } => {
            // `TurboQuantization` / `TurboQuantQuantizationConfig` /
            // `TurboQuantBitSize` are not re-exported by qdrant-edge 0.8 and
            // have no public constructor, so the nested body is the only way in.
            let mut body = serde_json::Map::new();
            if let Some(bits) = turbo.bits.as_ref() {
                body.insert("bits".into(), serde_json::Value::String(bits.clone()));
            }
            if let Some(always_ram) = turbo.always_ram {
                body.insert("always_ram".into(), serde_json::Value::Bool(always_ram));
            }
            if let Some(memory) = turbo.memory {
                body.insert(
                    "memory".into(),
                    serde_json::Value::String(memory.as_str().to_string()),
                );
            }
            serde_json::from_value(serde_json::json!({ "turbo": body })).map_err(|error| {
                edge_config_error(format!("invalid turbo quantization configuration: {error}"))
            })
        }
    }
}

fn edge_compression(value: &str) -> Result<CompressionRatio, QqlError> {
    match value {
        "x4" => Ok(CompressionRatio::X4),
        "x8" => Ok(CompressionRatio::X8),
        "x16" => Ok(CompressionRatio::X16),
        "x32" => Ok(CompressionRatio::X32),
        "x64" => Ok(CompressionRatio::X64),
        other => Err(edge_config_error(format!(
            "product quantization compression '{other}' is not supported (expected x4, x8, x16, x32, or x64)"
        ))),
    }
}

fn edge_binary_encoding(value: &str) -> Result<BinaryQuantizationEncoding, QqlError> {
    match value {
        "one_bit" => Ok(BinaryQuantizationEncoding::OneBit),
        "two_bits" => Ok(BinaryQuantizationEncoding::TwoBits),
        "one_and_half_bits" => Ok(BinaryQuantizationEncoding::OneAndHalfBits),
        other => Err(edge_config_error(format!(
            "binary quantization encoding '{other}' is not supported (expected one_bit, two_bits, or one_and_half_bits)"
        ))),
    }
}

fn edge_binary_query_encoding(value: &str) -> Result<BinaryQuantizationQueryEncoding, QqlError> {
    match value {
        "default" => Ok(BinaryQuantizationQueryEncoding::Default),
        "binary" => Ok(BinaryQuantizationQueryEncoding::Binary),
        "scalar4bits" => Ok(BinaryQuantizationQueryEncoding::Scalar4Bits),
        "scalar8bits" => Ok(BinaryQuantizationQueryEncoding::Scalar8Bits),
        other => Err(edge_config_error(format!(
            "binary query encoding '{other}' is not supported (expected default, binary, scalar4bits, or scalar8bits)"
        ))),
    }
}

/// Map the plan's dense/sparse datatype onto the engine's storage datatype.
fn edge_datatype(datatype: VectorDatatype) -> qdrant_edge::VectorStorageDatatype {
    match datatype {
        VectorDatatype::Float32 => qdrant_edge::VectorStorageDatatype::Float32,
        VectorDatatype::Float16 => qdrant_edge::VectorStorageDatatype::Float16,
        VectorDatatype::Uint8 => qdrant_edge::VectorStorageDatatype::Uint8,
        VectorDatatype::Turbo4 => qdrant_edge::VectorStorageDatatype::Turbo4,
    }
}

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
fn resolve_on_disk(on_disk: Option<bool>, memory: Option<MemoryPlacement>) -> Option<bool> {
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

fn usize_config(field: &str, value: u64) -> Result<usize, QqlError> {
    usize::try_from(value)
        .map_err(|error| edge_config_error(format!("{field} is too large: {error}")))
}

fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partial_hnsw(m: Option<u64>) -> HnswConfig {
        HnswConfig {
            m,
            ef_construct: None,
            full_scan_threshold: None,
            max_indexing_threads: None,
            on_disk: None,
            payload_m: None,
            inline_storage: None,
            memory: None,
        }
    }

    fn empty_optimizers() -> OptimizersConfig {
        OptimizersConfig {
            deleted_threshold: None,
            vacuum_min_vector_number: None,
            default_segment_number: None,
            max_segment_size: None,
            memmap_threshold: None,
            indexing_threshold: None,
            flush_interval_sec: None,
            max_optimization_threads: None,
            prevent_unoptimized: None,
        }
    }

    /// A partial `WITH HNSW (m = …)` must merge over engine defaults instead of
    /// failing on the engine's required fields.
    #[test]
    fn partial_hnsw_config_merges_engine_defaults() {
        let edge = edge_hnsw_config(&partial_hnsw(Some(32))).expect("partial hnsw");
        assert_eq!(edge.m, 32);
        assert_eq!(
            edge.ef_construct,
            qdrant_edge::HnswIndexConfig::default().ef_construct
        );
    }

    #[test]
    fn optimizer_partial_config_converts() {
        let mut config = empty_optimizers();
        config.indexing_threshold = Some(500);
        let edge = edge_optimizers_config(&config).expect("optimizer");
        assert_eq!(edge.indexing_threshold, Some(500));
    }

    /// Keys qdrant-edge deliberately excludes fail closed rather than being
    /// silently dropped.
    #[test]
    fn optimizer_excluded_keys_fail_closed() {
        let mut config = empty_optimizers();
        config.flush_interval_sec = Some(5);
        let error = edge_optimizers_config(&config).expect_err("flush interval");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY");
        assert_eq!(error.field("config_key"), Some("flush_interval_sec"));

        let mut config = empty_optimizers();
        config.memmap_threshold = Some(100);
        assert_eq!(
            edge_optimizers_config(&config).unwrap_err().code,
            "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY"
        );

        let mut config = empty_optimizers();
        config.max_optimization_threads = Some(qql_plan::MaxOptimizationThreads::Threads(4));
        assert_eq!(
            edge_optimizers_config(&config).unwrap_err().code,
            "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY"
        );
    }

    fn hnsw(
        m: Option<u64>,
        ef_construct: Option<u64>,
        memory: Option<MemoryPlacement>,
    ) -> HnswConfig {
        HnswConfig {
            m,
            ef_construct,
            full_scan_threshold: None,
            max_indexing_threads: None,
            on_disk: None,
            payload_m: None,
            inline_storage: None,
            memory,
        }
    }

    fn scalar_quantization(memory: MemoryPlacement) -> qql_plan::QuantizationConfig {
        qql_plan::QuantizationConfig::Scalar {
            scalar: qql_plan::ScalarQuantization {
                qtype: "int8".to_string(),
                quantile: Some(0.99),
                always_ram: None,
                memory: Some(memory),
            },
        }
    }

    fn dense(
        on_disk: Option<bool>,
        memory: Option<MemoryPlacement>,
        datatype: Option<VectorDatatype>,
        quantization: Option<qql_plan::QuantizationConfig>,
        per_vector_hnsw: Option<HnswConfig>,
    ) -> DenseVectorParams {
        DenseVectorParams {
            size: 8,
            distance: VectorDistance::Cosine,
            hnsw_config: per_vector_hnsw,
            quantization_config: quantization,
            on_disk,
            memory,
            datatype,
            multivector_config: None,
        }
    }

    /// Per-vector storage/datatype/quantization settings reach the engine
    /// instead of being dropped on the floor.
    #[test]
    fn per_vector_config_is_passed_through() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(
            "dense".to_string(),
            dense(
                Some(true),
                None,
                Some(VectorDatatype::Float16),
                Some(scalar_quantization(MemoryPlacement::Cached)),
                None,
            ),
        );
        let req = CreateCollectionRequest {
            vectors: Some(DenseVectorsConfig::Named(vectors)),
            ..Default::default()
        };
        let config = build_edge_config(&req, false).expect("edge config");
        let params = config.vectors.get("dense").expect("dense vector");
        assert_eq!(params.on_disk, Some(true));
        assert_eq!(
            params.datatype,
            Some(qdrant_edge::VectorStorageDatatype::Float16)
        );
        match params
            .quantization_config
            .as_ref()
            .expect("per-vector quantization")
        {
            qdrant_edge::QuantizationConfig::Scalar(scalar) => {
                assert_eq!(scalar.scalar.quantile, Some(0.99));
            }
            other => panic!("expected scalar quantization, got {other:?}"),
        }
    }

    /// `memory` maps onto the engine's RAM/mmap switch with `pinned` → RAM and
    /// `cached`/`cold` → mmap, and wins over the deprecated `on_disk` flag.
    #[test]
    fn memory_maps_to_engine_on_disk() {
        assert_eq!(
            resolve_on_disk(None, Some(MemoryPlacement::Pinned)),
            Some(false)
        );
        assert_eq!(
            resolve_on_disk(None, Some(MemoryPlacement::Cached)),
            Some(true)
        );
        assert_eq!(
            resolve_on_disk(None, Some(MemoryPlacement::Cold)),
            Some(true)
        );
        assert_eq!(resolve_on_disk(Some(true), None), Some(true));
        assert_eq!(resolve_on_disk(None, None), None);
        // Qdrant 1.19: `memory` overrides the deprecated flag.
        assert_eq!(
            resolve_on_disk(Some(false), Some(MemoryPlacement::Cold)),
            Some(true)
        );
    }

    /// A per-vector HNSW block is field-merged over the collection-wide config
    /// (the engine treats it as a full replacement, not a diff).
    #[test]
    fn per_vector_hnsw_merges_over_global() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(
            "dense".to_string(),
            dense(None, None, None, None, Some(hnsw(Some(32), None, None))),
        );
        let req = CreateCollectionRequest {
            vectors: Some(DenseVectorsConfig::Named(vectors)),
            hnsw_config: Some(hnsw(None, Some(200), None)),
            ..Default::default()
        };
        let config = build_edge_config(&req, false).expect("edge config");
        assert_eq!(config.hnsw_config().ef_construct, 200);
        let per_vector = config
            .vectors
            .get("dense")
            .and_then(|params| params.hnsw_config)
            .expect("per-vector HNSW");
        assert_eq!(per_vector.m, 32);
        assert_eq!(per_vector.ef_construct, 200);
    }

    /// Sparse index options (threshold, memory, datatype) are wired through;
    /// the engine stores sparse weights as mmap and uses `on_disk` only for
    /// the index placement.
    #[test]
    fn sparse_config_is_passed_through() {
        let mut sparse = std::collections::BTreeMap::new();
        sparse.insert(
            "text".to_string(),
            SparseVectorParams {
                index: Some(qql_plan::SparseIndexParams {
                    full_scan_threshold: Some(5_000),
                    on_disk: None,
                    memory: Some(MemoryPlacement::Pinned),
                    datatype: Some(VectorDatatype::Float16),
                }),
                modifier: qql_plan::SparseModifier::Idf,
            },
        );
        let req = CreateCollectionRequest {
            sparse_vectors: Some(sparse),
            ..Default::default()
        };
        let config = build_edge_config(&req, false).expect("edge config");
        let params = config.sparse_vectors.get("text").expect("sparse vector");
        assert_eq!(params.full_scan_threshold, Some(5_000));
        assert_eq!(params.on_disk, Some(false));
        assert_eq!(
            params.datatype,
            Some(qdrant_edge::VectorStorageDatatype::Float16)
        );
        assert_eq!(params.modifier, Some(qdrant_edge::Modifier::Idf));
    }

    /// Collection-wide `WITH HNSW (memory = pinned)` reaches the engine enum.
    #[test]
    fn hnsw_memory_is_passed_through() {
        let req = CreateCollectionRequest {
            vectors: Some(DenseVectorsConfig::Single(dense(
                None, None, None, None, None,
            ))),
            hnsw_config: Some(hnsw(None, None, Some(MemoryPlacement::Pinned))),
            ..Default::default()
        };
        let config = build_edge_config(&req, false).expect("edge config");
        let expected = qdrant_edge::HnswIndexConfig {
            memory: Some(edge_memory(MemoryPlacement::Pinned).expect("pinned")),
            ..qdrant_edge::HnswIndexConfig::default()
        };
        assert_eq!(config.hnsw_config().memory, expected.memory);
    }

    /// Unknown scalar/product keywords fail closed instead of a serde drop.
    #[test]
    fn quantization_unknown_keywords_fail_closed() {
        let error = edge_quantization_config(&qql_plan::QuantizationConfig::Scalar {
            scalar: qql_plan::ScalarQuantization {
                qtype: "float32".into(),
                quantile: None,
                always_ram: None,
                memory: None,
            },
        })
        .expect_err("bad scalar type");
        assert_eq!(error.code, "QQL-EDGE-CONFIG");
        assert!(error.message.contains("int8"), "{}", error.message);

        let error = edge_quantization_config(&qql_plan::QuantizationConfig::Product {
            product: qql_plan::ProductQuantization {
                compression: "x2".into(),
                always_ram: None,
                memory: None,
            },
        })
        .expect_err("bad compression");
        assert_eq!(error.code, "QQL-EDGE-CONFIG");
        assert!(error.message.contains("x4"), "{}", error.message);
    }

    /// `turbo4` is dense-only; a sparse index requesting it fails closed.
    #[test]
    fn sparse_turbo4_fails_closed() {
        let mut sparse = std::collections::BTreeMap::new();
        sparse.insert(
            "text".to_string(),
            SparseVectorParams {
                index: Some(qql_plan::SparseIndexParams {
                    full_scan_threshold: None,
                    on_disk: None,
                    memory: None,
                    datatype: Some(VectorDatatype::Turbo4),
                }),
                modifier: qql_plan::SparseModifier::None,
            },
        );
        let req = CreateCollectionRequest {
            sparse_vectors: Some(sparse),
            ..Default::default()
        };
        let error = build_edge_config(&req, false).expect_err("sparse turbo4");
        assert_eq!(error.code, "QQL-EDGE-CONFIG");
        assert!(error.message.contains("turbo4"), "{}", error.message);
    }
}
