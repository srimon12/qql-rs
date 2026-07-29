//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.

use std::collections::HashMap;

use qdrant_edge::EdgeConfigBuilder;

use qql::client::CreateCollectionReq;
use qql_core::error::QqlError;

pub(crate) fn build_edge_config(
    req: &CreateCollectionReq,
    on_disk_payload: bool,
) -> Result<qdrant_edge::EdgeConfig, QqlError> {
    let mut builder = EdgeConfigBuilder::new().on_disk_payload(on_disk_payload);

    if let Some(ref vc) = req.vectors_config {
        if let Ok(map) =
            serde_json::from_value::<HashMap<String, qdrant_edge::EdgeVectorParams>>(vc.clone())
        {
            for (name, params) in map {
                builder = builder.vector(name, params);
            }
        } else if let Ok(params) =
            serde_json::from_value::<qdrant_edge::EdgeVectorParams>(vc.clone())
        {
            builder = builder.vector(String::new(), params);
        } else if let Some(map) = vc.as_object() {
            for (name, params) in map {
                let size = params
                    .get("size")
                    .and_then(|value| value.as_u64())
                    .ok_or_else(|| {
                        edge_config_error(format!(
                            "vector '{name}' requires a positive integer size"
                        ))
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
                    // Edge only supports max_sim today.
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
        } else {
            return Err(edge_config_error("invalid dense vector configuration"));
        }
    }

    if let Some(ref svc) = req.sparse_vectors_config {
        if let Ok(map) = serde_json::from_value::<
            HashMap<String, qdrant_edge::EdgeSparseVectorParams>,
        >(svc.clone())
        {
            for (name, params) in map {
                builder = builder.sparse_vector(name, params);
            }
        } else {
            return Err(edge_config_error("invalid sparse vector configuration"));
        }
    }

    if let Some(ref hc) = req.hnsw_config {
        let hnsw = serde_json::from_value::<qdrant_edge::HnswIndexConfig>(hc.clone())
            .map_err(|error| edge_config_error(format!("invalid HNSW configuration: {error}")))?;
        builder = builder.hnsw_config(hnsw);
    }

    if let Some(ref qc) = req.quantization_config {
        let quant = serde_json::from_value::<qdrant_edge::QuantizationConfig>(qc.clone()).map_err(
            |error| edge_config_error(format!("invalid quantization configuration: {error}")),
        )?;
        builder = builder.quantization_config(quant);
    }

    if let Some(ref oc) = req.optimizers_config {
        let opt = serde_json::from_value::<qdrant_edge::EdgeOptimizersConfig>(oc.clone()).map_err(
            |error| edge_config_error(format!("invalid optimizer configuration: {error}")),
        )?;
        builder = builder.optimizers(opt);
    }

    Ok(builder.build())
}

fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}
