//! Transport-neutral QQL backend model.
//!
//! Types in this module are the boundary between QQL compilation and a Qdrant
//! transport adapter. They deliberately do not depend on generated OpenAPI or
//! protobuf types.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Qdrant point identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PointId {
    Num(u64),
    Uuid(String),
}

impl Serialize for PointId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Num(id) => serializer.serialize_u64(*id),
            Self::Uuid(id) => serializer.serialize_str(id),
        }
    }
}

impl<'de> Deserialize<'de> for PointId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WirePointId {
            Num(u64),
            Uuid(String),
            Legacy {
                num: Option<u64>,
                uuid: Option<String>,
            },
        }

        match WirePointId::deserialize(deserializer)? {
            WirePointId::Num(id) => Ok(Self::Num(id)),
            WirePointId::Uuid(id) => Ok(Self::Uuid(id)),
            WirePointId::Legacy {
                num: Some(id),
                uuid: None,
            } => Ok(Self::Num(id)),
            WirePointId::Legacy {
                num: None,
                uuid: Some(id),
            } => Ok(Self::Uuid(id)),
            WirePointId::Legacy { .. } => Err(serde::de::Error::custom(
                "point ID must contain exactly one of num or uuid",
            )),
        }
    }
}

/// A QQL filter in Qdrant's documented semantic JSON shape.
///
/// Filters remain opaque to the executor: each transport owns conversion from
/// this canonical representation to its wire format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Filter(pub serde_json::Value);

impl Filter {
    pub fn from_json(value: serde_json::Value) -> Self {
        Self(value)
    }

    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }

    pub fn into_json(self) -> serde_json::Value {
        self.0
    }
}

/// A transport-neutral point accepted by an upsert operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub id: PointId,
    pub vector: serde_json::Value,
    pub payload: HashMap<String, serde_json::Value>,
}

/// A point returned from a similarity query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredPoint {
    pub id: PointId,
    pub score: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
}

/// A point returned by retrieve or scroll operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedPoint {
    pub id: PointId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
}

/// A grouped query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointGroup {
    pub id: serde_json::Value,
    #[serde(default)]
    pub hits: Vec<ScoredPoint>,
}

/// The vector schema needed by QQL's topology resolution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionSchema {
    #[serde(default)]
    pub dense_vectors: Vec<String>,
    #[serde(default)]
    pub sparse_vectors: Vec<String>,
}

/// Transport-neutral collection metadata consumed by the executor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionInfo {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub points_count: u64,
    #[serde(default)]
    pub segments_count: u64,
    #[serde(default)]
    pub schema: CollectionSchema,
    #[serde(default)]
    pub raw_json: Option<serde_json::Value>,
}
