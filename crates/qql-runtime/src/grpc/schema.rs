//! Proto `CollectionInfo` → shared typed [`CollectionSchema`].
//!
//! Dual-reads the Qdrant 1.19-deprecated on-disk / always-ram fields while
//! also reporting the new `memory` placement. See `grpc_route`.

#![allow(deprecated)]

use crate::backend::{
    CollectionParamsSpec, CollectionSchema, PayloadIndexSpec, SparseVectorSpec, VectorSpec,
};
use crate::qdrant_grpc::qdrant;

use super::memory::memory_to_str;

/// Map gRPC collection info into the shared typed schema (no JSON round-trip).
pub(crate) fn schema_from_grpc_collection(info: &qdrant::CollectionInfo) -> CollectionSchema {
    let mut schema = CollectionSchema::default();

    if let Some(config) = &info.config {
        if let Some(params) = &config.params {
            if let Some(vc) = &params.vectors_config {
                match &vc.config {
                    Some(qdrant::vectors_config::Config::Params(p)) => {
                        schema.vectors.push(vector_params_to_spec(None, p));
                        schema.dense_vectors.clear();
                    }
                    Some(qdrant::vectors_config::Config::ParamsMap(map)) => {
                        let mut names: Vec<String> = map.map.keys().cloned().collect();
                        names.sort();
                        schema.dense_vectors = names.clone();
                        for name in &names {
                            if let Some(p) = map.map.get(name) {
                                schema
                                    .vectors
                                    .push(vector_params_to_spec(Some(name.clone()), p));
                            }
                        }
                    }
                    None => {}
                }
            }
            if let Some(sparse) = &params.sparse_vectors_config {
                let mut specs = Vec::new();
                for (name, p) in &sparse.map {
                    let modifier = p
                        .modifier
                        .and_then(|m| qdrant::Modifier::try_from(m).ok())
                        .map(|m| match m {
                            qdrant::Modifier::Idf => "idf".to_string(),
                            qdrant::Modifier::None => "none".to_string(),
                        });
                    let index = p.index.as_ref().map(|i| {
                        let mut map = serde_json::Map::new();
                        if let Some(v) = i.full_scan_threshold {
                            map.insert("full_scan_threshold".into(), serde_json::json!(v));
                        }
                        if let Some(v) = i.on_disk {
                            map.insert("on_disk".into(), serde_json::Value::Bool(v));
                        }
                        if let Some(v) = i.datatype {
                            if let Ok(dt) = qdrant::Datatype::try_from(v) {
                                // Normalize protobuf enum names to QQL/OpenAPI forms.
                                let name = match dt {
                                    qdrant::Datatype::Float32 => "float32",
                                    qdrant::Datatype::Uint8 => "uint8",
                                    qdrant::Datatype::Float16 => "float16",
                                    qdrant::Datatype::Turbo4 => "turbo4",
                                    qdrant::Datatype::Default => "default",
                                };
                                map.insert(
                                    "datatype".into(),
                                    serde_json::Value::String(name.into()),
                                );
                            }
                        }
                        if let Some(name) = i.memory.and_then(memory_to_str) {
                            map.insert("memory".into(), serde_json::Value::String(name.into()));
                        }
                        map
                    });
                    specs.push(SparseVectorSpec {
                        name: name.clone(),
                        index,
                        modifier,
                    });
                }
                specs.sort_by(|a, b| a.name.cmp(&b.name));
                schema.sparse_vectors = specs;
            }
            let sharding_method = params.sharding_method.and_then(|m| {
                qdrant::ShardingMethod::try_from(m).ok().map(|sm| match sm {
                    qdrant::ShardingMethod::Auto => "auto".to_string(),
                    qdrant::ShardingMethod::Custom => "custom".to_string(),
                })
            });
            schema.params = CollectionParamsSpec {
                shard_number: Some(params.shard_number as u64),
                sharding_method,
                on_disk_payload: Some(params.on_disk_payload),
                payload_memory: params
                    .payload
                    .as_ref()
                    .and_then(|p| p.memory)
                    .and_then(memory_to_str)
                    .map(str::to_string),
                replication_factor: params.replication_factor.map(|n| n as u64),
            };
        }
        if let Some(hnsw) = &config.hnsw_config {
            schema.hnsw = Some(hnsw_diff_to_map(hnsw));
        }
        if let Some(opt) = &config.optimizer_config {
            schema.optimizers = Some(optimizer_config_diff_to_map(opt));
        }
        if let Some(quant) = &config.quantization_config {
            schema.quantization = quantization_config_to_json(quant);
        }
    }

    for (field, meta) in &info.payload_schema {
        let data_type = qdrant::PayloadSchemaType::try_from(meta.data_type)
            .ok()
            .map(|t| t.as_str_name().to_ascii_lowercase())
            .unwrap_or_else(|| "keyword".into());
        let data_type = match data_type.as_str() {
            "unknowntype" => "keyword".to_string(),
            other => other.to_string(),
        };

        let (params_map, is_tenant) = payload_index_params_from_proto(meta.params.as_ref());
        schema.payload_indexes.push(PayloadIndexSpec {
            field: field.clone(),
            data_type,
            params: params_map,
            is_tenant,
        });
    }
    schema.payload_indexes.sort_by(|a, b| a.field.cmp(&b.field));

    schema
}

fn vector_params_to_spec(name: Option<String>, p: &qdrant::VectorParams) -> VectorSpec {
    let distance = qdrant::Distance::try_from(p.distance)
        .ok()
        .map(|d| d.as_str_name().to_string())
        .unwrap_or_else(|| "Cosine".into());
    let hnsw = p.hnsw_config.as_ref().map(hnsw_diff_to_map);
    let quantization = p
        .quantization_config
        .as_ref()
        .and_then(quantization_config_to_json);
    let multivector = p.multivector_config.as_ref().map(|m| {
        let mut map = serde_json::Map::new();
        if let Ok(c) = qdrant::MultiVectorComparator::try_from(m.comparator) {
            let name = match c {
                qdrant::MultiVectorComparator::MaxSim => "max_sim",
            };
            map.insert("comparator".into(), serde_json::Value::String(name.into()));
        }
        map
    });
    let datatype = p
        .datatype
        .and_then(|v| qdrant::Datatype::try_from(v).ok())
        .map(|dt| {
            match dt {
                qdrant::Datatype::Float32 => "float32",
                qdrant::Datatype::Uint8 => "uint8",
                qdrant::Datatype::Float16 => "float16",
                qdrant::Datatype::Turbo4 => "turbo4",
                qdrant::Datatype::Default => "default",
            }
            .to_string()
        });
    let memory = p.memory.and_then(memory_to_str).map(str::to_string);
    VectorSpec {
        name,
        size: p.size,
        distance,
        hnsw,
        quantization,
        multivector,
        on_disk: p.on_disk,
        datatype,
        memory,
    }
}

fn hnsw_diff_to_map(diff: &qdrant::HnswConfigDiff) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if let Some(v) = diff.m {
        map.insert("m".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.ef_construct {
        map.insert("ef_construct".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.full_scan_threshold {
        map.insert("full_scan_threshold".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.max_indexing_threads {
        map.insert("max_indexing_threads".into(), serde_json::Value::from(v));
    }
    if let Some(v) = diff.on_disk {
        map.insert("on_disk".into(), serde_json::Value::Bool(v));
    }
    if let Some(v) = diff.payload_m {
        map.insert("payload_m".into(), serde_json::Value::from(v));
    }
    if let Some(name) = diff.memory.and_then(memory_to_str) {
        map.insert("memory".into(), serde_json::Value::String(name.into()));
    }
    map
}

fn optimizer_config_diff_to_map(
    diff: &qdrant::OptimizersConfigDiff,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if let Some(v) = diff.deleted_threshold {
        map.insert("deleted_threshold".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.vacuum_min_vector_number {
        map.insert("vacuum_min_vector_number".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.default_segment_number {
        map.insert("default_segment_number".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.max_segment_size {
        map.insert("max_segment_size".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.indexing_threshold {
        map.insert("indexing_threshold".into(), serde_json::json!(v));
    }
    if let Some(v) = diff.flush_interval_sec {
        map.insert("flush_interval_sec".into(), serde_json::json!(v));
    }
    if let Some(ref mot) = diff.max_optimization_threads {
        if let Some(ref variant) = mot.variant {
            match variant {
                qdrant::max_optimization_threads::Variant::Value(val) => {
                    map.insert("max_optimization_threads".into(), serde_json::json!(val));
                }
                qdrant::max_optimization_threads::Variant::Setting(_) => {
                    map.insert("max_optimization_threads".into(), serde_json::json!("auto"));
                }
            }
        }
    }
    map
}

/// Convert protobuf quantization into the nested REST-shaped JSON that dump
/// understands (`{ "scalar": {...} }`, `{ "turbo": {...} }`, …).
fn quantization_config_to_json(q: &qdrant::QuantizationConfig) -> Option<serde_json::Value> {
    use qdrant::quantization_config::Quantization;
    match &q.quantization {
        Some(Quantization::Scalar(sq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("scalar"));
            if let Some(v) = sq.quantile {
                map.insert("quantile".into(), serde_json::json!(v));
            }
            if let Some(v) = sq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            Some(serde_json::json!({ "scalar": map }))
        }
        Some(Quantization::Product(pq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("product"));
            if let Ok(c) = qdrant::CompressionRatio::try_from(pq.compression) {
                map.insert(
                    "compression".into(),
                    serde_json::Value::String(c.as_str_name().to_string()),
                );
            }
            if let Some(v) = pq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            Some(serde_json::json!({ "product": map }))
        }
        Some(Quantization::Binary(bq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("binary"));
            if let Some(v) = bq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            if let Some(enc) = bq.encoding {
                if let Ok(e) = qdrant::BinaryQuantizationEncoding::try_from(enc) {
                    // QQL CREATE accepts snake_case aliases, not protobuf enum names.
                    let qql_enc = match e {
                        qdrant::BinaryQuantizationEncoding::OneBit => "one_bit",
                        qdrant::BinaryQuantizationEncoding::TwoBits => "two_bits",
                        qdrant::BinaryQuantizationEncoding::OneAndHalfBits => "one_and_half_bits",
                    };
                    map.insert("encoding".into(), serde_json::Value::String(qql_enc.into()));
                }
            }
            if let Some(ref qe) = bq.query_encoding {
                if let Some(qdrant::binary_quantization_query_encoding::Variant::Setting(s)) =
                    qe.variant
                {
                    if let Ok(setting) =
                        qdrant::binary_quantization_query_encoding::Setting::try_from(s)
                    {
                        let name = match setting {
                            qdrant::binary_quantization_query_encoding::Setting::Binary => "binary",
                            qdrant::binary_quantization_query_encoding::Setting::Scalar4Bits => {
                                "scalar4bits"
                            }
                            qdrant::binary_quantization_query_encoding::Setting::Scalar8Bits => {
                                "scalar8bits"
                            }
                            qdrant::binary_quantization_query_encoding::Setting::Default => {
                                "default"
                            }
                        };
                        map.insert(
                            "query_encoding".into(),
                            serde_json::Value::String(name.into()),
                        );
                    }
                }
            }
            Some(serde_json::json!({ "binary": map }))
        }
        Some(Quantization::Turboquant(tq)) => {
            let mut map = serde_json::Map::new();
            map.insert("type".into(), serde_json::json!("turbo"));
            if let Some(v) = tq.always_ram {
                map.insert("always_ram".into(), serde_json::Value::Bool(v));
            }
            if let Some(bits) = tq.bits {
                if let Ok(b) = qdrant::TurboQuantBitSize::try_from(bits) {
                    // Numeric form matches QQL CREATE: bits = 1 | 1.5 | 2 | 4
                    let n = match b {
                        qdrant::TurboQuantBitSize::Bits1 => 1.0,
                        qdrant::TurboQuantBitSize::Bits15 => 1.5,
                        qdrant::TurboQuantBitSize::Bits2 => 2.0,
                        qdrant::TurboQuantBitSize::Bits4 => 4.0,
                    };
                    map.insert("bits".into(), serde_json::json!(n));
                }
            }
            Some(serde_json::json!({ "turbo": map }))
        }
        None => None,
    }
}

/// Extract dump-relevant index options from protobuf params (best-effort).
fn payload_index_params_from_proto(
    params: Option<&qdrant::PayloadIndexParams>,
) -> (serde_json::Map<String, serde_json::Value>, Option<bool>) {
    let mut map = serde_json::Map::new();
    let mut is_tenant = None;
    let Some(params) = params else {
        return (map, is_tenant);
    };
    use qdrant::payload_index_params::IndexParams;
    match &params.index_params {
        Some(IndexParams::KeywordIndexParams(p)) => {
            is_tenant = p.is_tenant;
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if p.prefix.is_some() {
                map.insert("prefix".into(), serde_json::Value::Bool(true));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        Some(IndexParams::UuidIndexParams(p)) => {
            is_tenant = p.is_tenant;
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
        }
        Some(IndexParams::TextIndexParams(p)) => {
            if let Some(v) = p.lowercase {
                map.insert("lowercase".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Ok(tok) = qdrant::TokenizerType::try_from(p.tokenizer) {
                let name = tok.as_str_name().to_ascii_lowercase();
                if name != "unknowntokenizer" {
                    map.insert("tokenizer".into(), serde_json::Value::String(name));
                }
            }
            if let Some(v) = p.min_token_len {
                map.insert("min_token_len".into(), serde_json::Value::from(v));
            }
            if let Some(v) = p.max_token_len {
                map.insert("max_token_len".into(), serde_json::Value::from(v));
            }
        }
        Some(IndexParams::IntegerIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.is_principal {
                map.insert("is_principal".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        Some(IndexParams::FloatIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        Some(IndexParams::DatetimeIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        Some(IndexParams::GeoIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        Some(IndexParams::BoolIndexParams(p)) => {
            if let Some(v) = p.on_disk {
                map.insert("on_disk".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = p.memory.and_then(memory_to_str) {
                map.insert("memory".into(), serde_json::Value::String(v.into()));
            }
        }
        None => {}
    }
    (map, is_tenant)
}
