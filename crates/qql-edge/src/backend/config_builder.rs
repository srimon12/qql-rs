//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.

use qdrant_edge::EdgeConfigBuilder;

use qql_core::ast::VectorDistance;
use qql_core::error::QqlError;
use qql_plan::{CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig};

pub(crate) fn build_edge_config(
    req: &CreateCollectionRequest,
    on_disk_payload: bool,
) -> Result<qdrant_edge::EdgeConfig, QqlError> {
    let mut builder = EdgeConfigBuilder::new().on_disk_payload(on_disk_payload);

    match &req.vectors {
        Some(DenseVectorsConfig::Single(params)) => {
            builder = builder.vector(String::new(), edge_vector_params("<default>", params)?);
        }
        Some(DenseVectorsConfig::Named(map)) => {
            for (name, params) in map {
                builder = builder.vector(name.clone(), edge_vector_params(name, params)?);
            }
        }
        None => {}
    }

    if let Some(ref sparse) = req.sparse_vectors {
        for (name, params) in sparse {
            builder = builder.sparse_vector(
                name.clone(),
                qdrant_edge::EdgeSparseVectorParams {
                    full_scan_threshold: params
                        .index
                        .as_ref()
                        .and_then(|index| index.full_scan_threshold)
                        .map(|n| n as usize),
                    on_disk: params.index.as_ref().and_then(|index| index.on_disk),
                    modifier: Some(match params.modifier {
                        qql_plan::SparseModifier::Idf => qdrant_edge::Modifier::Idf,
                        qql_plan::SparseModifier::None => qdrant_edge::Modifier::None,
                    }),
                    // qdrant-edge stores sparse weights as f32 regardless of the
                    // requested datatype; leaving this unset matches its default.
                    datatype: None,
                },
            );
        }
    }

    if let Some(ref hc) = req.hnsw_config {
        let val = serde_json::to_value(hc).map_err(|e| edge_config_error(e.to_string()))?;
        let hnsw = serde_json::from_value::<qdrant_edge::HnswIndexConfig>(val)
            .map_err(|error| edge_config_error(format!("invalid HNSW configuration: {error}")))?;
        builder = builder.hnsw_config(hnsw);
    }

    if let Some(ref qc) = req.quantization_config {
        let val = serde_json::to_value(qc).map_err(|e| edge_config_error(e.to_string()))?;
        let quant =
            serde_json::from_value::<qdrant_edge::QuantizationConfig>(val).map_err(|error| {
                edge_config_error(format!("invalid quantization configuration: {error}"))
            })?;
        builder = builder.quantization_config(quant);
    }

    if let Some(ref oc) = req.optimizers_config {
        let val = serde_json::to_value(oc).map_err(|e| edge_config_error(e.to_string()))?;
        let opt =
            serde_json::from_value::<qdrant_edge::EdgeOptimizersConfig>(val).map_err(|error| {
                edge_config_error(format!("invalid optimizer configuration: {error}"))
            })?;
        builder = builder.optimizers(opt);
    }

    Ok(builder.build())
}

fn edge_vector_params(
    name: &str,
    params: &DenseVectorParams,
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
    Ok(qdrant_edge::EdgeVectorParams {
        size,
        distance,
        multivector_config,
        // qdrant-edge 0.8 does not surface datatype / per-vector HNSW /
        // quantization / on_disk through the create config builder yet.
        datatype: None,
        hnsw_config: None,
        quantization_config: None,
        on_disk: None,
    })
}

fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}
