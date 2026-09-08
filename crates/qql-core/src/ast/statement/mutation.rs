//! Typed AST for mutation statements (UPSERT, DELETE, UPDATE, etc.).

use super::types::*;
use crate::ast::{FilterExpr, Value};
use alloc::string::String;
use alloc::vec::Vec;

/// Role of an `EMBED` directive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbedKind {
    /// Dense embedding (the default role).
    Dense {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Sparse (e.g. BM25) embedding.
    Sparse {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Multivector / ColBERT bag (`embed_multi` → MultiDense).
    Multi {
        /// Optional embedding model override.
        model: Option<String>,
    },
    /// Image / CLIP vision path or URL → dense vector (`embed_image`).
    Image {
        /// Optional embedding model override.
        model: Option<String>,
    },
}

/// `EMBED <field> INTO <vector> [USING …]` directive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EmbedDirective {
    /// Payload field providing the embedding input.
    pub source_field: String,
    /// Named vector to write into.
    pub target_vector: String,
    /// Embedding role and model for this directive.
    pub kind: EmbedKind,
}

/// Upsert-level `USING` embedding clause.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbeddingSpec {
    /// `USING DENSE` / `USING MODEL` / `USING VECTOR` — dense embedding.
    Dense {
        /// Optional embedding model.
        model: Option<String>,
        /// Optional target vector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// `USING SPARSE` — sparse embedding.
    Sparse {
        /// Optional sparse model (e.g. BM25-compatible).
        model: Option<String>,
        /// Optional target vector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// `USING HYBRID` — parallel dense + sparse embedding.
    Hybrid {
        /// Optional dense embedding model.
        dense_model: Option<String>,
        /// Optional dense target vector name.
        dense_vector: Option<String>,
        /// Optional dense input payload field.
        dense_field: Option<String>,
        /// Optional sparse embedding model.
        sparse_model: Option<String>,
        /// Optional sparse target vector name.
        sparse_vector: Option<String>,
        /// Optional sparse input payload field.
        sparse_field: Option<String>,
    },
    /// Multivector / ColBERT: text → bag of token vectors for a named multi slot.
    MultiVector {
        /// Optional multivector embedding model.
        model: Option<String>,
        /// Optional target multivector name.
        vector: Option<String>,
        /// Optional payload field to embed.
        field: Option<String>,
    },
    /// Image / CLIP vision: payload field holds a path or URL → dense vector.
    Image {
        /// Optional CLIP vision model.
        model: Option<String>,
        /// Optional target dense vector name.
        vector: Option<String>,
        /// Optional payload field holding the image path or URL.
        field: Option<String>,
    },
    /// Combined specs (e.g. DENSE + SPARSE + MULTI VECTOR colbert).
    Multi(Vec<EmbeddingSpec>),
}

/// One `VALUES {…}` object of an `UPSERT INTO`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertPoint {
    /// Point identifier (unsigned integer or string).
    pub id: PointId,
    /// Optional pre-computed vectors, unnamed or by name.
    pub vectors: Option<PointVectors>,
    /// Remaining object entries as payload key-value pairs.
    pub payload: Vec<(String, Value)>,
}

/// One entry of an `UPSERT INTO … VALUES` list: either an inline point
/// object or a whole-point placeholder (`:name` / `?`) bound later to a
/// point dict (or a list of point dicts, splicing several points).
///
/// `untagged` keeps the JSON shape of inline points identical to before,
/// so existing AST snapshots are unaffected.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum PointEntry {
    /// Inline `{id: …, …}` point object.
    Inline(UpsertPoint),
    /// Named whole-point placeholder (`:name`).
    Param(
        String,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<alloc::boxed::Box<crate::error::Span>>,
    ),
    /// Positional whole-point placeholder (`?`).
    PositionalParam(
        usize,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<alloc::boxed::Box<crate::error::Span>>,
    ),
}

impl PointEntry {
    /// Borrow the inline point, if this entry is one (`VALUES {…}` rows
    /// always are; placeholders become inline once bound).
    pub fn as_inline(&self) -> Option<&UpsertPoint> {
        match self {
            PointEntry::Inline(point) => Some(point),
            PointEntry::Param(..) | PointEntry::PositionalParam(..) => None,
        }
    }
}

/// `UPSERT INTO <collection> VALUES …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpsertStmt {
    /// Target collection.
    pub collection: String,
    /// Points to upsert.
    pub points: Vec<PointEntry>,
    /// Optional `USING` embedding clause.
    pub embedding: Option<EmbeddingSpec>,
    /// `EMBED <field> INTO <vector>` directives.
    pub embed: Vec<EmbedDirective>,
    /// `SHARD '<key>'` or `SHARD <n>` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `CLEAR PAYLOAD FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClearPayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Points whose payload is cleared.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `DELETE VECTOR <names> FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteVectorStmt {
    /// Target collection.
    pub collection: String,
    /// Points whose named vectors are removed.
    pub selector: PointSelector,
    /// Named vectors to remove.
    pub vector_names: Vec<String>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// Point selection used by mutation statements.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointSelector {
    /// A single point ID.
    Id(PointId),
    /// An explicit list of point IDs.
    Ids(Vec<PointId>),
    /// All points matching a filter.
    Filter(Box<FilterExpr>),
}

/// `DELETE FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeleteStmt {
    /// Target collection.
    pub collection: String,
    /// Points to delete.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `UPDATE <collection> SET VECTOR … WHERE id = …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdateVectorStmt {
    /// Target collection.
    pub collection: String,
    /// Point whose vector is replaced.
    pub point_id: PointId,
    /// New vector value.
    pub vector: VectorValue,
    /// Named vector to update; `None` targets the unnamed vector.
    pub vector_name: Option<String>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `DELETE PAYLOAD <keys> FROM <collection> WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeletePayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Payload keys to remove.
    pub keys: Vec<String>,
    /// Points whose payload keys are removed.
    pub selector: PointSelector,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}

/// `UPDATE <collection> SET PAYLOAD = {…} WHERE …` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UpdatePayloadStmt {
    /// Target collection.
    pub collection: String,
    /// Points to update.
    pub selector: PointSelector,
    /// Payload keys to merge into the points.
    pub payload: Vec<(String, Value)>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional write durability confirmation (`WAIT true` / `WAIT false`).
    pub wait: Option<bool>,
}
