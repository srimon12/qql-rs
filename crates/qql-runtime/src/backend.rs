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
    /// Numeric point ID, serialized as Qdrant's `num` field.
    Num(u64),
    /// UUID point ID, serialized as Qdrant's `uuid` field.
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
    /// Wrap an already-shaped Qdrant filter JSON value.
    pub fn from_json(value: serde_json::Value) -> Self {
        Self(value)
    }

    /// Borrow the underlying filter JSON value.
    pub fn as_json(&self) -> &serde_json::Value {
        &self.0
    }

    /// Consume the filter and return its JSON value.
    pub fn into_json(self) -> serde_json::Value {
        self.0
    }
}

/// A transport-neutral point accepted by an upsert operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Point identifier (`num` integer or `uuid` string).
    pub id: PointId,
    /// Vector value in Qdrant wire shape (array, named map, or sparse object).
    pub vector: serde_json::Value,
    /// Payload key/value map stored with the point.
    pub payload: HashMap<String, serde_json::Value>,
}

/// A point returned from a similarity query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredPoint {
    /// Matched point identifier.
    pub id: PointId,
    /// Similarity score assigned by the backend.
    pub score: f32,
    /// Payload when requested via `WITH PAYLOAD`; absent when stripped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
    /// Vector when requested via `WITH VECTOR`; absent by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
}

/// A point returned by retrieve or scroll operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedPoint {
    /// Retrieved point identifier.
    pub id: PointId,
    /// Payload when requested via `WITH PAYLOAD`; absent when stripped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
    /// Vector when requested via `WITH VECTOR`; absent by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
}

/// A grouped query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointGroup {
    /// Group key value as returned by Qdrant (JSON-typed).
    pub id: serde_json::Value,
    /// Scored points in this group, in backend order.
    #[serde(default)]
    pub hits: Vec<ScoredPoint>,
}

/// One dense vector definition from collection config.
///
/// `name == None` means Qdrant's default unnamed vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorSpec {
    /// Vector name; `None` for the unnamed default vector.
    pub name: Option<String>,
    /// Vector dimension.
    pub size: u64,
    /// Distance metric as reported by Qdrant (e.g. `Cosine`, `Dot`).
    pub distance: String,
    /// Per-vector HNSW overrides (`hnsw_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hnsw: Option<serde_json::Map<String, serde_json::Value>>,
    /// Per-vector quantization config (`quantization_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<serde_json::Value>,
    /// Multivector (ColBERT) config (`multivector_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multivector: Option<serde_json::Map<String, serde_json::Value>>,
    /// Whether vector storage stays on disk (`on_disk`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Storage element type (`datatype`, e.g. `float32`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    /// Memory placement hint for vector storage (`memory`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// A payload field index declared on the collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayloadIndexSpec {
    /// Payload field path the index is built on.
    pub field: String,
    /// Indexed data type (e.g. `keyword`, `integer`, `datetime`).
    pub data_type: String,
    /// Extra index parameters (e.g. full-text analyzer settings).
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// Whether this is a tenant index (`is_tenant`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_tenant: Option<bool>,
}

/// Collection-level params relevant to dump / DDL reconstruction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionParamsSpec {
    /// Number of shards (`shard_number`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    /// Sharding method name (`sharding_method`, e.g. `auto`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<String>,
    /// Whether payloads are stored on disk (`on_disk_payload`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk_payload: Option<bool>,
    /// Payload memory placement (`params.payload.memory`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_memory: Option<String>,
    /// Shard replication factor (`replication_factor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replication_factor: Option<u64>,
}

/// A named sparse vector definition from collection config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparseVectorSpec {
    /// Sparse vector name as declared on the collection.
    pub name: String,
    /// Sparse index settings (`index`), e.g. on-disk placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<serde_json::Map<String, serde_json::Value>>,
    /// Optional modifier (`modifier`, e.g. `idf` for server-side BM25 IDF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<String>,
}

/// The vector / index schema needed by topology resolution and dump.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionSchema {
    /// Named dense vector names. Empty for a single unnamed default vector.
    #[serde(default)]
    pub dense_vectors: Vec<String>,
    /// Named sparse vector definitions.
    #[serde(default)]
    pub sparse_vectors: Vec<SparseVectorSpec>,
    /// Full dense vector definitions (size + distance) when the backend provides them.
    #[serde(default)]
    pub vectors: Vec<VectorSpec>,
    /// Payload field indexes declared on the collection.
    #[serde(default)]
    pub payload_indexes: Vec<PayloadIndexSpec>,
    /// Collection-level parameters relevant to dump / DDL reconstruction.
    #[serde(default)]
    pub params: CollectionParamsSpec,
    /// Collection-level HNSW config (`hnsw_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hnsw: Option<serde_json::Map<String, serde_json::Value>>,
    /// Collection-level optimizer config (`optimizer_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimizers: Option<serde_json::Map<String, serde_json::Value>>,
    /// Collection-level quantization config (`quantization_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<serde_json::Value>,
}

/// Transport-neutral collection metadata consumed by the executor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionInfo {
    /// Collection health status reported by the backend (e.g. `green`).
    #[serde(default)]
    pub status: String,
    /// Number of points currently stored in the collection.
    #[serde(default)]
    pub points_count: u64,
    /// Number of storage segments backing the collection.
    #[serde(default)]
    pub segments_count: u64,
    /// Vector and index schema used for `USING` resolution and dumps.
    #[serde(default)]
    pub schema: CollectionSchema,
}

/// Parse Qdrant REST `/collections/{name}` result JSON into a typed schema.
///
/// Shared by the REST adapter so dump and topology checks never dig through
/// untyped JSON on their own.
pub fn schema_from_rest_result(result: &serde_json::Value) -> CollectionSchema {
    let mut schema = CollectionSchema::default();
    let params = result.get("config").and_then(|c| c.get("params"));

    if let Some(vectors) = params
        .and_then(|p| p.get("vectors"))
        .and_then(|v| v.as_object())
    {
        // Unnamed default vector: { "size": N, "distance": "Cosine" }
        if vectors.contains_key("size") && vectors.contains_key("distance") {
            let vec_obj = serde_json::Value::Object(vectors.clone());
            if let Some(spec) = extract_vector_spec(None, &vec_obj) {
                schema.vectors.push(spec);
            }
            schema.dense_vectors.clear();
        } else {
            let mut named = Vec::new();
            for (name, cfg) in vectors {
                if is_pseudo_vector_key(name) {
                    continue;
                }
                named.push(name.clone());
                if let Some(spec) = extract_vector_spec(Some(name.clone()), cfg) {
                    schema.vectors.push(spec);
                }
            }
            named.sort();
            schema.vectors.sort_by(|a, b| a.name.cmp(&b.name));
            schema.dense_vectors = named;
        }
    }

    if let Some(sparse) = params
        .and_then(|p| p.get("sparse_vectors"))
        .and_then(|v| v.as_object())
    {
        for (name, cfg) in sparse {
            let modifier = cfg
                .get("modifier")
                .and_then(|m| m.as_str())
                .map(String::from);
            let index = cfg.get("index").and_then(|i| i.as_object()).cloned();
            schema.sparse_vectors.push(SparseVectorSpec {
                name: name.clone(),
                index,
                modifier,
            });
        }
        schema.sparse_vectors.sort_by(|a, b| a.name.cmp(&b.name));
    }

    if let Some(p) = params {
        schema.params.shard_number = p.get("shard_number").and_then(|v| v.as_u64());
        schema.params.sharding_method = p
            .get("sharding_method")
            .and_then(|v| v.as_str())
            .map(String::from);
        schema.params.on_disk_payload = p.get("on_disk_payload").and_then(|v| v.as_bool());
        schema.params.replication_factor = p.get("replication_factor").and_then(|v| v.as_u64());
        schema.params.payload_memory = result
            .get("config")
            .and_then(|c| c.get("params"))
            .and_then(|p| p.get("payload"))
            .and_then(|payload| payload.get("memory"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }

    if let Some(payload_schema) = result.get("payload_schema").and_then(|s| s.as_object()) {
        for (field, meta) in payload_schema {
            let data_type = meta
                .get("data_type")
                .and_then(|t| t.as_str())
                .or_else(|| meta.get("type").and_then(|t| t.as_str()))
                .unwrap_or("keyword")
                .to_ascii_lowercase();

            let mut params_map = serde_json::Map::new();
            if let Some(obj) = meta.get("params").and_then(|p| p.as_object()) {
                for (k, v) in obj {
                    if k != "type" {
                        params_map.insert(k.clone(), v.clone());
                    }
                }
            }
            let is_tenant = meta
                .get("is_tenant")
                .and_then(|v| v.as_bool())
                .or_else(|| params_map.get("is_tenant").and_then(|v| v.as_bool()));

            schema.payload_indexes.push(PayloadIndexSpec {
                field: field.clone(),
                data_type,
                params: params_map,
                is_tenant,
            });
        }
        schema.payload_indexes.sort_by(|a, b| a.field.cmp(&b.field));
    }

    if let Some(config_obj) = result.get("config").and_then(|c| c.as_object()) {
        schema.hnsw = config_obj
            .get("hnsw_config")
            .and_then(|h| h.as_object())
            .map(filter_hnsw_map);
        // Qdrant REST historically uses optimizer_config (singular); accept both.
        schema.optimizers = config_obj
            .get("optimizer_config")
            .or_else(|| config_obj.get("optimizers_config"))
            .and_then(|o| o.as_object())
            .map(filter_optimizers_map);
        schema.quantization = config_obj.get("quantization_config").cloned();
    }

    schema
}

/// Keep only HNSW keys QQL can re-parse.
fn filter_hnsw_map(
    map: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    const KEYS: &[&str] = &[
        "m",
        "ef_construct",
        "full_scan_threshold",
        "max_indexing_threads",
        "on_disk",
        "payload_m",
        "inline_storage",
        "memory",
    ];
    filter_known_keys(map, KEYS)
}

/// Keep only OPTIMIZERS keys QQL can re-parse.
fn filter_optimizers_map(
    map: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    const KEYS: &[&str] = &[
        "deleted_threshold",
        "vacuum_min_vector_number",
        "default_segment_number",
        "max_segment_size",
        "memmap_threshold",
        "indexing_threshold",
        "flush_interval_sec",
        "max_optimization_threads",
        "prevent_unoptimized",
    ];
    filter_known_keys(map, KEYS)
}

fn filter_known_keys(
    map: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for key in keys {
        if let Some(val) = map.get(*key)
            && !val.is_null()
        {
            out.insert((*key).to_string(), val.clone());
        }
    }
    out
}

fn is_pseudo_vector_key(name: &str) -> bool {
    matches!(
        name,
        "size"
            | "distance"
            | "hnsw_config"
            | "quantization_config"
            | "multivector_config"
            | "on_disk"
            | "datatype"
    )
}

fn extract_vector_spec(name: Option<String>, cfg: &serde_json::Value) -> Option<VectorSpec> {
    let size = cfg.get("size").and_then(|s| s.as_u64())?;
    let distance = cfg
        .get("distance")
        .and_then(|d| d.as_str())
        .unwrap_or("Cosine")
        .to_string();
    let hnsw = cfg.get("hnsw_config").and_then(|h| h.as_object()).cloned();
    let quantization = cfg.get("quantization_config").cloned();
    let multivector = cfg
        .get("multivector_config")
        .and_then(|m| m.as_object())
        .cloned();
    let on_disk = cfg.get("on_disk").and_then(|b| b.as_bool());
    let datatype = cfg
        .get("datatype")
        .and_then(|d| d.as_str())
        .map(String::from);
    let memory = cfg.get("memory").and_then(|m| m.as_str()).map(String::from);

    Some(VectorSpec {
        name,
        size,
        distance,
        hnsw,
        quantization,
        multivector,
        on_disk,
        datatype,
        memory,
    })
}
