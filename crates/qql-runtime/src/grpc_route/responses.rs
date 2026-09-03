//! Proto responses → REST-shaped JSON envelopes.
//!
//! Keeps the executor's hit extraction working unchanged regardless of
//! transport (`result.points` for queries, mutation envelopes with timing).

#![allow(deprecated)]

use crate::grpc::memory::memory_to_str;
use crate::qdrant_grpc::qdrant;

use super::values::qdrant_value_to_json;

pub(crate) fn point_id_to_json(id: &qdrant::PointId) -> serde_json::Value {
    match &id.point_id_options {
        Some(qdrant::point_id::PointIdOptions::Num(n)) => serde_json::json!(*n),
        Some(qdrant::point_id::PointIdOptions::Uuid(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

pub(crate) fn group_id_to_json(id: &qdrant::GroupId) -> serde_json::Value {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => serde_json::json!(*n),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(qdrant::group_id::Kind::StringValue(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

pub(crate) fn vector_output_to_json(vo: &qdrant::VectorOutput) -> serde_json::Value {
    use qdrant::vector_output;
    match &vo.vector {
        Some(vector_output::Vector::Dense(d)) => {
            serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
        }
        Some(vector_output::Vector::Sparse(s)) => serde_json::json!({
            "indices": s.indices,
            "values": s.values,
        }),
        Some(vector_output::Vector::MultiDense(m)) => serde_json::Value::Array(
            m.vectors
                .iter()
                .map(|d| {
                    serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
                })
                .collect(),
        ),
        None => serde_json::Value::Null,
    }
}

pub(crate) fn vectors_output_to_json(v: &qdrant::VectorsOutput) -> serde_json::Value {
    use qdrant::vectors_output::VectorsOptions;
    match &v.vectors_options {
        Some(VectorsOptions::Vector(vo)) => vector_output_to_json(vo),
        Some(VectorsOptions::Vectors(named)) => {
            let mut map = serde_json::Map::new();
            for (name, vec) in &named.vectors {
                map.insert(name.clone(), vector_output_to_json(vec));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

pub(crate) fn scored_point_to_json(p: qdrant::ScoredPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("score".into(), serde_json::json!(p.score));
    obj.insert("payload".into(), payload);
    if p.version != 0 {
        obj.insert("version".into(), serde_json::json!(p.version));
    }
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

pub(crate) fn retrieved_point_to_json(p: qdrant::RetrievedPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("payload".into(), payload);
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

pub(crate) fn groups_result_to_json(r: qdrant::GroupsResult) -> serde_json::Value {
    serde_json::json!({
        "groups": r.groups.into_iter().map(point_group_to_json).collect::<Vec<_>>(),
    })
}

pub(crate) fn batch_result_to_json(r: qdrant::BatchResult) -> serde_json::Value {
    let points: Vec<_> = r.result.into_iter().map(scored_point_to_json).collect();
    serde_json::json!({ "result": { "points": points } })
}

pub(crate) fn point_group_to_json(g: qdrant::PointGroup) -> serde_json::Value {
    let hits: Vec<_> = g.hits.into_iter().map(scored_point_to_json).collect();
    let id =
        g.id.as_ref()
            .map_or(serde_json::Value::Null, group_id_to_json);
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("hits".into(), serde_json::json!(hits));
    if let Some(lookup) = g.lookup {
        obj.insert("lookup".into(), retrieved_point_to_json(lookup));
    }
    serde_json::Value::Object(obj)
}

pub(crate) fn list_collections_response_to_json(
    resp: qdrant::ListCollectionsResponse,
) -> serde_json::Value {
    serde_json::json!({
        "result": {
            "collections": resp.collections.into_iter()
                .map(|c| serde_json::json!({"name": c.name}))
                .collect::<Vec<_>>(),
        },
        "status": "ok",
        "time": resp.time,
    })
}

pub(crate) fn collection_info_to_json(
    resp: qdrant::GetCollectionInfoResponse,
) -> serde_json::Value {
    let info = resp.result.map(|info| {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), serde_json::json!(info.status));
        if let Some(os) = info.optimizer_status {
            obj.insert("optimizer_status".into(), serde_json::json!(os.ok));
        }
        obj.insert(
            "segments_count".into(),
            serde_json::json!(info.segments_count),
        );
        if let Some(pc) = info.points_count {
            obj.insert("points_count".into(), serde_json::json!(pc));
        }
        if let Some(ivc) = info.indexed_vectors_count {
            obj.insert("indexed_vectors_count".into(), serde_json::json!(ivc));
        }
        if let Some(cfg) = info.config {
            obj.insert("config".into(), collection_config_to_json(&cfg));
        }
        if !info.payload_schema.is_empty() {
            let schema: serde_json::Map<_, _> = info
                .payload_schema
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "data_type": v.data_type,
                            "points": v.points,
                        }),
                    )
                })
                .collect();
            obj.insert("payload_schema".into(), serde_json::Value::Object(schema));
        }
        serde_json::Value::Object(obj)
    });
    serde_json::json!({
        "result": info,
        "status": "ok",
        "time": resp.time,
    })
}

pub(crate) fn collection_config_to_json(c: &qdrant::CollectionConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(params) = &c.params {
        let mut p = serde_json::Map::new();
        p.insert(
            "shard_number".into(),
            serde_json::json!(params.shard_number),
        );
        p.insert(
            "on_disk_payload".into(),
            serde_json::json!(params.on_disk_payload),
        );
        if let Some(vc) = &params.vectors_config {
            p.insert("vectors".into(), vectors_config_to_json(vc));
        }
        if let Some(rf) = params.replication_factor {
            p.insert("replication_factor".into(), serde_json::json!(rf));
        }
        if let Some(wcf) = params.write_consistency_factor {
            p.insert("write_consistency_factor".into(), serde_json::json!(wcf));
        }
        if let Some(rff) = params.read_fan_out_factor {
            p.insert("read_fan_out_factor".into(), serde_json::json!(rff));
        }
        if let Some(svc) = &params.sparse_vectors_config {
            let map: serde_json::Map<_, _> = svc
                .map
                .iter()
                .map(|(k, v)| {
                    let mut entry = serde_json::Map::new();
                    if let Some(sidx) = &v.index {
                        entry.insert(
                            "index".into(),
                            serde_json::json!({
                                "on_disk": sidx.on_disk,
                            }),
                        );
                    }
                    (k.clone(), serde_json::Value::Object(entry))
                })
                .collect();
            p.insert("sparse_vectors".into(), serde_json::Value::Object(map));
        }
        obj.insert("params".into(), serde_json::Value::Object(p));
    }
    if let Some(hnsw) = &c.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(opt) = &c.optimizer_config {
        let max_threads = opt
            .max_optimization_threads
            .as_ref()
            .map(|m| match &m.variant {
                Some(qdrant::max_optimization_threads::Variant::Value(n)) => {
                    serde_json::json!(*n)
                }
                Some(qdrant::max_optimization_threads::Variant::Setting(_)) => {
                    serde_json::json!("auto")
                }
                None => serde_json::Value::Null,
            });
        obj.insert(
            "optimizer_config".into(),
            serde_json::json!({
                "deleted_threshold": opt.deleted_threshold,
                "vacuum_min_vector_number": opt.vacuum_min_vector_number,
                "default_segment_number": opt.default_segment_number,
                "max_segment_size": opt.max_segment_size,
                "memmap_threshold": opt.memmap_threshold,
                "indexing_threshold": opt.indexing_threshold,
                "flush_interval_sec": opt.flush_interval_sec,
                "max_optimization_threads": max_threads,
            }),
        );
    }
    if let Some(wal) = &c.wal_config {
        obj.insert(
            "wal_config".into(),
            serde_json::json!({
                "wal_capacity_mb": wal.wal_capacity_mb,
                "wal_segments_ahead": wal.wal_segments_ahead,
            }),
        );
    }
    if let Some(qc) = &c.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_response_to_json(qc),
        );
    }
    if let Some(sm) = &c.strict_mode_config {
        obj.insert(
            "strict_mode_config".into(),
            serde_json::json!({
                "enabled": sm.enabled,
                "max_collection_vector_size_bytes": sm.max_collection_vector_size_bytes,
                "read_rate_limit": sm.read_rate_limit,
                "write_rate_limit": sm.write_rate_limit,
                "max_query_limit": sm.max_query_limit,
            }),
        );
    }
    serde_json::Value::Object(obj)
}

pub(crate) fn vectors_config_to_json(vc: &qdrant::VectorsConfig) -> serde_json::Value {
    use qdrant::vectors_config::Config;
    match &vc.config {
        Some(Config::Params(p)) => vector_params_to_json(p),
        Some(Config::ParamsMap(pm)) => {
            let map: serde_json::Map<_, _> = pm
                .map
                .iter()
                .map(|(k, v)| (k.clone(), vector_params_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::json!({}),
    }
}

pub(crate) fn vector_params_to_json(vp: &qdrant::VectorParams) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("size".into(), serde_json::json!(vp.size));
    obj.insert(
        "distance".into(),
        serde_json::json!(distance_to_str(vp.distance)),
    );
    if let Some(od) = vp.on_disk {
        obj.insert("on_disk".into(), serde_json::json!(od));
    }
    if let Some(dt) = vp.datatype.and_then(|v| qdrant::Datatype::try_from(v).ok()) {
        let name = match dt {
            qdrant::Datatype::Float32 => "float32",
            qdrant::Datatype::Uint8 => "uint8",
            qdrant::Datatype::Float16 => "float16",
            qdrant::Datatype::Turbo4 => "turbo4",
            qdrant::Datatype::Default => "float32",
        };
        obj.insert("datatype".into(), serde_json::json!(name));
    }
    if let Some(m) = vp.memory.and_then(memory_to_str) {
        obj.insert("memory".into(), serde_json::json!(m));
    }
    if let Some(hnsw) = &vp.hnsw_config {
        let mut hnsw_map = serde_json::Map::new();
        hnsw_map.insert("m".into(), serde_json::json!(hnsw.m));
        hnsw_map.insert("ef_construct".into(), serde_json::json!(hnsw.ef_construct));
        hnsw_map.insert(
            "full_scan_threshold".into(),
            serde_json::json!(hnsw.full_scan_threshold),
        );
        hnsw_map.insert(
            "max_indexing_threads".into(),
            serde_json::json!(hnsw.max_indexing_threads),
        );
        hnsw_map.insert("on_disk".into(), serde_json::json!(hnsw.on_disk));
        hnsw_map.insert("payload_m".into(), serde_json::json!(hnsw.payload_m));
        if let Some(m) = hnsw.memory.and_then(memory_to_str) {
            hnsw_map.insert("memory".into(), serde_json::json!(m));
        }
        obj.insert("hnsw_config".into(), serde_json::Value::Object(hnsw_map));
    }
    if let Some(qc) = &vp.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_response_to_json(qc),
        );
    }
    if let Some(mv) = &vp.multivector_config {
        obj.insert(
            "multivector_config".into(),
            serde_json::json!({
                "comparator": multivec_comp_to_str(mv.comparator),
            }),
        );
    }
    serde_json::Value::Object(obj)
}

pub(crate) fn distance_to_str(d: i32) -> &'static str {
    match d {
        1 => "Cosine",
        2 => "Euclid",
        3 => "Dot",
        4 => "Manhattan",
        _ => "UnknownDistance",
    }
}

pub(crate) fn multivec_comp_to_str(c: i32) -> &'static str {
    match c {
        0 => "MaxSim",
        _ => "MaxSim",
    }
}

/// Build a REST-shaped mutation envelope from a gRPC `PointsOperationResponse`.
pub(crate) fn mutation_response_from(resp: qdrant::PointsOperationResponse) -> serde_json::Value {
    let result = resp
        .result
        .map(update_result_to_json)
        .unwrap_or_else(|| serde_json::json!({ "status": "completed" }));
    serde_json::json!({
        "result": result,
        "status": "ok",
        "time": resp.time,
    })
}

/// REST-shaped envelope for collection-level mutations (create/update/drop).
pub(crate) fn collection_mutation_response(
    resp: qdrant::CollectionOperationResponse,
) -> serde_json::Value {
    serde_json::json!({
        "result": resp.result,
        "status": "ok",
        "time": resp.time,
    })
}

/// Fallback when the gRPC response type carries no timing (shard-key ops).
pub(crate) fn mutation_response_ok() -> serde_json::Value {
    serde_json::json!({
        "result": { "status": "completed" },
        "status": "ok",
        "time": 0.0_f64,
    })
}

pub(crate) fn update_result_to_json(r: qdrant::UpdateResult) -> serde_json::Value {
    let status = match r.status() {
        qdrant::UpdateStatus::Acknowledged => "acknowledged",
        qdrant::UpdateStatus::Completed => "completed",
        qdrant::UpdateStatus::ClockRejected => "clock_rejected",
        qdrant::UpdateStatus::WaitTimeout => "wait_timeout",
        qdrant::UpdateStatus::UnknownUpdateStatus => "unknown",
    };
    serde_json::json!({
        "operation_id": r.operation_id,
        "status": status,
    })
}

/// REST-compatible envelope for `GetPoints`: hit extraction reads
/// `result.points`, so a bare `result` array would silently drop every hit
/// (B-3 regression).
pub(crate) fn get_points_envelope(points: Vec<serde_json::Value>, time: f64) -> serde_json::Value {
    serde_json::json!({
        "result": { "points": points },
        "status": "ok",
        "time": time,
    })
}

pub(crate) fn quantization_response_to_json(qc: &qdrant::QuantizationConfig) -> serde_json::Value {
    use qdrant::quantization_config::Quantization;
    let mut obj = serde_json::Map::new();
    match &qc.quantization {
        Some(Quantization::Scalar(s)) => {
            let mut scalar = serde_json::Map::new();
            scalar.insert("r#type".into(), serde_json::json!(s.r#type));
            scalar.insert("quantile".into(), serde_json::json!(s.quantile));
            scalar.insert("always_ram".into(), serde_json::json!(s.always_ram));
            if let Some(m) = s.memory.and_then(memory_to_str) {
                scalar.insert("memory".into(), serde_json::json!(m));
            }
            obj.insert("scalar".into(), serde_json::Value::Object(scalar));
        }
        Some(Quantization::Product(p)) => {
            let mut product = serde_json::Map::new();
            product.insert("compression".into(), serde_json::json!(p.compression));
            product.insert("always_ram".into(), serde_json::json!(p.always_ram));
            if let Some(m) = p.memory.and_then(memory_to_str) {
                product.insert("memory".into(), serde_json::json!(m));
            }
            obj.insert("product".into(), serde_json::Value::Object(product));
        }
        Some(Quantization::Binary(b)) => {
            let mut binary = serde_json::Map::new();
            binary.insert("always_ram".into(), serde_json::json!(b.always_ram));
            if let Some(m) = b.memory.and_then(memory_to_str) {
                binary.insert("memory".into(), serde_json::json!(m));
            }
            obj.insert("binary".into(), serde_json::Value::Object(binary));
        }
        Some(Quantization::Turboquant(_)) => {}
        None => {}
    }
    serde_json::Value::Object(obj)
}
