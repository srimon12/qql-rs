//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.

use std::collections::HashMap;

use qdrant_edge::EdgeConfigBuilder;

use qql_core::error::QqlError;
use qql_plan::CreateCollectionRequest;

pub(crate) fn build_edge_config(
    req: &CreateCollectionRequest,
    on_disk_payload: bool,
) -> Result<qdrant_edge::EdgeConfig, QqlError> {
    let mut builder = EdgeConfigBuilder::new().on_disk_payload(on_disk_payload);

    let vectors_map = req
        .vectors
        .as_ref()
        .or_else(|| req.vectors_config.as_ref().and_then(|v| v.as_object()));

    if let Some(map) = vectors_map {
        for (name, params) in map {
            let size = params
                .get("size")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| {
                    edge_config_error(format!("vector '{name}' requires a positive integer size"))
                })?;
            let size = usize::try_from(size).map_err(|error| {
                edge_config_error(format!("vector '{name}' size is too large: {error}"))
            })?;
            let distance = match params.get("distance").and_then(|v| v.as_str()) {
                Some("Cosine") | Some("cosine") => qdrant_edge::Distance::Cosine,
                Some("Dot") | Some("dot") => qdrant_edge::Distance::Dot,
                Some("Euclid") | Some("euclid") => qdrant_edge::Distance::Euclid,
                Some("Manhattan") | Some("manhattan") => qdrant_edge::Distance::Manhattan,
                Some(other) => {
                    return Err(edge_config_error(format!(
                        "vector '{name}' has unsupported distance '{other}'"
                    )))
                }
                None => {
                    return Err(edge_config_error(format!(
                        "vector '{name}' requires a distance"
                    )))
                }
            };
            let multivector_config = params.get("multivector_config").and_then(|mv| {
                let comparator = mv
                    .get("comparator")
                    .and_then(|c| c.as_str())
                    .unwrap_or("max_sim");
                if comparator.eq_ignore_ascii_case("max_sim") {
                    Some(qdrant_edge::MultiVectorConfig {
                        comparator: qdrant_edge::MultiVectorComparator::MaxSim,
                    })
                } else {
                    None
                }
            });
            let edge_params = qdrant_edge::EdgeVectorParams {
                size,
                distance,
                multivector_config,
                datatype: None,
                hnsw_config: None,
                quantization_config: None,
                on_disk: None,
            };
            if name.is_empty() {
                builder = builder.vector(String::new(), edge_params);
            } else {
                builder = builder.vector(name.clone(), edge_params);
            }
        }
    }

    if let Some(ref svc) = req.sparse_vectors {
        let val = serde_json::to_value(svc).map_err(|e| edge_config_error(e.to_string()))?;
        if let Ok(map) =
            serde_json::from_value::<HashMap<String, qdrant_edge::EdgeSparseVectorParams>>(val)
        {
            for (name, params) in map {
                builder = builder.sparse_vector(name, params);
            }
        } else {
            return Err(edge_config_error("invalid sparse vector configuration"));
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

fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}
