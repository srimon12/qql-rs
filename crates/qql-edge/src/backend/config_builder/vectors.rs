//! Collection builder: `CreateCollectionRequest` → engine `EdgeConfig`.
//!
//! Pure move from `config_builder.rs` (size hygiene split). HNSW/optimizer
//! lowering lives in [`super::hnsw`], quantization in [`super::quantization`].

use qdrant_edge::EdgeConfigBuilder;
use qql_core::ast::{VectorDatatype, VectorDistance};
use qql_core::error::QqlError;
use qql_plan::{
    CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig, SparseVectorParams,
};

use super::hnsw::{edge_hnsw_config, edge_hnsw_config_over, edge_optimizers_config};
use super::quantization::{edge_datatype, edge_quantization_config};
use super::shared::{edge_config_error, resolve_on_disk};
use crate::backend::unsupported::EdgeUnsupported;

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
