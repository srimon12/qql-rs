//! Collection create/alter lowering: statements to transport-neutral requests.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::collections::BTreeMap;
use alloc::string::String;
use qql_core::ast::{
    AlterCollectionStmt, CollectionConfig, CreateCollectionStmt, MultivectorComparator,
    VectorsConfig,
};
use qql_core::error::QqlError;

use super::collection_config_error;
use super::diff::{
    insert_sparse_vector_diff, insert_vector_diff, lower_quantization_diff,
    lower_quantization_update_diff, lower_sparse_index_params, lower_sparse_modifier,
    lower_storage_diff,
};
use super::options::{lower_collection_params, lower_metadata_map, lower_sharding_method};
use super::options::{lower_strict_mode_config, lower_wal_config};
use super::runtime::{lower_hnsw_config, lower_optimizers_config, lower_quantization_config};

/// Lower `CREATE COLLECTION` to the transport-neutral create request.
pub fn lower_create_collection(
    stmt: &CreateCollectionStmt,
) -> Result<CreateCollectionRequest, QqlError> {
    let mut req = CreateCollectionRequest {
        vectors: None,
        sparse_vectors: None,
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
        wal_config: None,
        strict_mode_config: None,
        metadata: None,
        shard_number: None,
        sharding_method: None,
        shard_keys: None,
    };

    let mut vectors = BTreeMap::new();
    for vd in &stmt.vectors {
        let params = DenseVectorParams {
            size: vd.size,
            distance: vd.distance,
            hnsw_config: vd.hnsw.as_deref().map(lower_hnsw_config),
            quantization_config: vd.quantization.as_deref().map(lower_quantization_config),
            on_disk: vd.vectors.as_deref().and_then(|cfg| cfg.on_disk),
            memory: vd.vectors.as_deref().and_then(|cfg| cfg.memory),
            datatype: vd.vectors.as_deref().and_then(|cfg| cfg.datatype),
            multivector_config: vd.multivector.as_ref().map(|mv| MultiVectorConfig {
                comparator: match mv.comparator {
                    MultivectorComparator::MaxSim => MultiVectorComparator::MaxSim,
                },
            }),
        };
        vectors.insert(vd.name.clone(), params);
    }

    let mut sparse = BTreeMap::new();
    for sv in &stmt.sparse_vectors {
        sparse.insert(
            sv.name.clone(),
            SparseVectorParams {
                index: sv.index.as_deref().map(lower_sparse_index_params),
                modifier: sv
                    .modifier
                    .as_deref()
                    .map(lower_sparse_modifier)
                    .unwrap_or(SparseModifier::Idf),
            },
        );
    }
    if !vectors.is_empty() {
        req.vectors = Some(DenseVectorsConfig::Named(vectors));
    }
    if !sparse.is_empty() {
        req.sparse_vectors = Some(sparse);
    }

    if let Some(ref config) = stmt.config {
        fill_collection_config(&mut req, config)?;
        // Collection-scoped `WITH VECTOR (…)` settings are defaults: they fill
        // unset per-vector values and never override an explicit per-vector
        // setting.
        if let Some(ref default) = config.vectors {
            apply_vector_defaults(req.vectors.as_mut(), default);
        }
    }

    Ok(req)
}

/// Lower `ALTER COLLECTION` to the transport-neutral update request.
pub fn lower_alter_collection(
    stmt: &AlterCollectionStmt,
) -> Result<UpdateCollectionRequest, QqlError> {
    let mut req = UpdateCollectionRequest {
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
        strict_mode_config: None,
        metadata: None,
        vectors: None,
        sparse_vectors: None,
    };
    if let Some(ref config) = stmt.config {
        fill_update_collection_config(&mut req, config)?;
    }
    Ok(req)
}

fn fill_collection_config(
    req: &mut CreateCollectionRequest,
    config: &CollectionConfig,
) -> Result<(), QqlError> {
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config(o));
    }
    if let Some(ref p) = config.params {
        req.params = Some(lower_collection_params(p));
        req.shard_number = p.shard_number;
        req.sharding_method = p.sharding_method.as_deref().map(lower_sharding_method);
        req.shard_keys = p.shard_keys.as_ref().map(|keys| {
            keys.iter()
                .map(crate::semantic::PlanShardKey::from)
                .collect()
        });
    }
    if let Some(ref q) = config.quantization {
        req.quantization_config = Some(lower_quantization_config(q));
    }
    if let Some(ref wal) = config.wal {
        req.wal_config = Some(Box::new(lower_wal_config(wal)?));
    }
    if let Some(ref strict) = config.strict_mode {
        req.strict_mode_config = Some(Box::new(lower_strict_mode_config(strict)?));
    }
    if let Some(ref metadata) = config.metadata {
        req.metadata = Some(lower_metadata_map(metadata));
    }
    Ok(())
}

fn fill_update_collection_config(
    req: &mut UpdateCollectionRequest,
    config: &CollectionConfig,
) -> Result<(), QqlError> {
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config(o));
    }
    if let Some(ref p) = config.params {
        req.params = Some(lower_collection_params(p));
    }
    req.quantization_config = lower_quantization_diff(config);
    if let Some(ref strict) = config.strict_mode {
        req.strict_mode_config = Some(Box::new(lower_strict_mode_config(strict)?));
    }
    if let Some(ref metadata) = config.metadata {
        req.metadata = Some(lower_metadata_map(metadata));
    }
    // The update wire shape has no WAL field; the parser rejects
    // `ALTER … WITH WAL`, so a hand-built AST reaching here fails closed.
    if let Some(pairs) = config.wal.as_deref() {
        let span = pairs.first().and_then(|(_, value)| value.param_span());
        return Err(collection_config_error(
            "WITH WAL is supported only for CREATE COLLECTION",
            span,
        ));
    }

    // `ALTER COLLECTION … WITH VECTOR (…)` addresses the default unnamed vector
    // (REST documents the empty-string map key for exactly that case).
    if let Some(ref storage) = config.vectors {
        let diff = lower_storage_diff(storage)?;
        insert_vector_diff(&mut req.vectors, String::new(), diff)?;
    }
    for diff in &config.vector_diffs {
        let mut params = match diff.vectors.as_deref() {
            Some(storage) => lower_storage_diff(storage)?,
            None => VectorParamsDiff::default(),
        };
        params.hnsw_config = diff.hnsw.as_deref().map(lower_hnsw_config);
        params.quantization_config = lower_quantization_update_diff(diff.quantization.as_deref());
        insert_vector_diff(&mut req.vectors, diff.name.clone(), params)?;
    }
    for diff in &config.sparse_vector_diffs {
        let params = SparseVectorParamsDiff {
            index: diff.index.as_deref().map(lower_sparse_index_params),
            modifier: diff.modifier.as_deref().map(lower_sparse_modifier),
        };
        insert_sparse_vector_diff(&mut req.sparse_vectors, diff.name.clone(), params)?;
    }
    Ok(())
}

fn apply_vector_defaults(vectors: Option<&mut DenseVectorsConfig>, default: &VectorsConfig) {
    fn apply(params: &mut DenseVectorParams, default: &VectorsConfig) {
        params.on_disk = params.on_disk.or(default.on_disk);
        params.memory = params.memory.or(default.memory);
        params.datatype = params.datatype.or(default.datatype);
    }
    match vectors {
        Some(DenseVectorsConfig::Single(params)) => apply(params, default),
        Some(DenseVectorsConfig::Named(map)) => {
            for params in map.values_mut() {
                apply(params, default);
            }
        }
        None => {}
    }
}
