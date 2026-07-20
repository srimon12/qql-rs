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
                let size = params.get("size").and_then(|v| v.as_u64()).unwrap_or(128) as usize;
                let distance = match params.get("distance").and_then(|v| v.as_str()) {
                    Some("Cosine") | Some("cosine") => qdrant_edge::Distance::Cosine,
                    Some("Dot") | Some("dot") => qdrant_edge::Distance::Dot,
                    _ => qdrant_edge::Distance::Euclid,
                };
                let edge_params = qdrant_edge::EdgeVectorParams {
                    size,
                    distance,
                    multivector_config: None,
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
    }

    if let Some(ref svc) = req.sparse_vectors_config {
        if let Ok(map) = serde_json::from_value::<
            HashMap<String, qdrant_edge::EdgeSparseVectorParams>,
        >(svc.clone())
        {
            for (name, params) in map {
                builder = builder.sparse_vector(name, params);
            }
        }
    }

    if let Some(ref hc) = req.hnsw_config {
        if let Ok(hnsw) = serde_json::from_value::<qdrant_edge::HnswIndexConfig>(hc.clone()) {
            builder = builder.hnsw_config(hnsw);
        }
    }

    if let Some(ref qc) = req.quantization_config {
        if let Ok(quant) = serde_json::from_value::<qdrant_edge::QuantizationConfig>(qc.clone()) {
            builder = builder.quantization_config(quant);
        }
    }

    if let Some(ref oc) = req.optimizers_config {
        if let Ok(opt) = serde_json::from_value::<qdrant_edge::EdgeOptimizersConfig>(oc.clone()) {
            builder = builder.optimizers(opt);
        }
    }

    Ok(builder.build())
}
