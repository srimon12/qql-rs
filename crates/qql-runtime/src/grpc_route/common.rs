//! Small shard-key / enum helpers shared by the plan-to-gRPC converters.

use qql_plan::{IndexFieldType, PlanPointId, PlanShardKey};

use crate::qdrant_grpc::qdrant;

pub(crate) fn shard_key_proto(key: &PlanShardKey) -> qdrant::ShardKey {
    qdrant::ShardKey {
        key: Some(match key {
            PlanShardKey::Keyword(s) => qdrant::shard_key::Key::Keyword(s.clone()),
            PlanShardKey::Number(n) => qdrant::shard_key::Key::Number(*n),
        }),
    }
}

pub(crate) fn shard_key_selector(key: &Option<PlanShardKey>) -> Option<qdrant::ShardKeySelector> {
    key.as_ref().map(|k| qdrant::ShardKeySelector {
        shard_keys: vec![shard_key_proto(k)],
        ..Default::default()
    })
}

/// Map a typed payload field schema onto the protobuf `FieldType` enum.
pub(crate) fn field_type_to_proto(field_type: IndexFieldType) -> i32 {
    match field_type {
        IndexFieldType::Keyword => qdrant::FieldType::Keyword as i32,
        IndexFieldType::Integer => qdrant::FieldType::Integer as i32,
        IndexFieldType::Float => qdrant::FieldType::Float as i32,
        IndexFieldType::Geo => qdrant::FieldType::Geo as i32,
        IndexFieldType::Text => qdrant::FieldType::Text as i32,
        IndexFieldType::Bool => qdrant::FieldType::Bool as i32,
        IndexFieldType::Datetime => qdrant::FieldType::Datetime as i32,
        IndexFieldType::Uuid => qdrant::FieldType::Uuid as i32,
    }
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
