use super::hnsw::{edge_hnsw_config, edge_optimizers_config};
use super::quantization::edge_quantization_config;
use super::shared::resolve_on_disk;
use super::*;
use qql_core::ast::{MemoryPlacement, VectorDatatype, VectorDistance};
use qql_plan::{
    CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig, HnswConfig, OptimizersConfig,
    SparseVectorParams,
};

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

fn hnsw(m: Option<u64>, ef_construct: Option<u64>, memory: Option<MemoryPlacement>) -> HnswConfig {
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
