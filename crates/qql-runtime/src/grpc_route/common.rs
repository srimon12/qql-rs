//! Small JSON / shard-key helpers shared by the plan-to-gRPC converters.

use qql_plan::PlanPointId;

use crate::qdrant_grpc::qdrant;

pub(crate) fn shard_key_selector(key: &Option<String>) -> Option<qdrant::ShardKeySelector> {
    key.as_ref().map(|k| qdrant::ShardKeySelector {
        shard_keys: vec![qdrant::ShardKey {
            key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
        }],
        ..Default::default()
    })
}

pub(crate) fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

pub(crate) fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(serde_json::Value::as_bool)
}

pub(crate) fn distance(value: &serde_json::Value) -> i32 {
    match value
        .get("distance")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Cosine")
        .to_ascii_lowercase()
        .as_str()
    {
        "euclid" => qdrant::Distance::Euclid as i32,
        "dot" => qdrant::Distance::Dot as i32,
        "manhattan" => qdrant::Distance::Manhattan as i32,
        _ => qdrant::Distance::Cosine as i32,
    }
}

/// Map OpenAPI / JSON datatype strings onto the protobuf `Datatype` enum.
pub(crate) fn datatype_from_json(value: &serde_json::Value) -> Option<i32> {
    value
        .get("datatype")
        .and_then(serde_json::Value::as_str)
        .map(|dt| match dt.to_ascii_lowercase().as_str() {
            "float32" | "f32" => qdrant::Datatype::Float32 as i32,
            "uint8" | "u8" => qdrant::Datatype::Uint8 as i32,
            "float16" | "f16" => qdrant::Datatype::Float16 as i32,
            "turbo4" | "t4" => qdrant::Datatype::Turbo4 as i32,
            _ => qdrant::Datatype::Default as i32,
        })
}

pub(crate) fn option_bool(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<bool> {
    options.get(key).and_then(serde_json::Value::as_bool)
}

pub(crate) fn option_u64(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<u64> {
    options.get(key).and_then(serde_json::Value::as_u64)
}

pub(crate) fn option_string(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    options
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn to_point_id(id: &PlanPointId) -> qdrant::PointId {
    match id {
        PlanPointId::Number(n) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(*n)),
        },
        PlanPointId::String(s) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(s.clone())),
        },
    }
}

/// Read an index `memory` option into the proto `Memory` enum value.
pub(crate) fn option_memory(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<i32> {
    options
        .get(key)
        .and_then(serde_json::Value::as_str)
        .and_then(crate::grpc::memory::memory_from_str)
}
