//! Mutation, scroll, count, and facet IR request types.

use crate::filter_types::FilterExpression;
use crate::query_types::{OrderByQuery, PayloadSelectorReq, VectorSelectorReq};
use crate::semantic::{PlanPointId, PlanPointVectors};
use alloc::string::String;
use alloc::vec::Vec;
use serde::Serialize;

/// Wire body for Qdrant's `POST /collections/{c}/points/batch` (mutation batch).
/// Maps to OpenAPI `UpdateOperations` / gRPC `UpdateBatchPoints`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateBatchRequest {
    /// Ordered mutation operations executed in a single backend call.
    pub operations: Vec<UpdateOperation>,
}

/// One entry in `UpdateOperations.operations` — OpenAPI `UpdateOperation`.
/// Each variant is a single-key object matching the wire format exactly.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UpdateOperation {
    /// `{ "upsert": … }` — insert or overwrite points.
    Upsert {
        /// Points to insert or overwrite.
        upsert: UpsertRequest,
    },
    /// `{ "delete": … }` — remove points by ID or filter.
    Delete {
        /// Selector for the points to remove.
        delete: DeleteRequest,
    },
    /// `{ "set_payload": … }` — merge payload keys.
    SetPayload {
        /// Payload keys to merge into the targeted points.
        set_payload: UpdatePayloadRequest,
    },
    /// `{ "overwrite_payload": … }` — replace the full payload.
    Overwrite {
        /// Payload replacing the targeted points' payload.
        overwrite_payload: UpdatePayloadRequest,
    },
    /// `{ "clear_payload": … }` — remove all payload.
    ClearPayload {
        /// Selector for the points whose payload is cleared.
        clear_payload: ClearPayloadRequest,
    },
    /// `{ "delete_payload": … }` — remove payload keys.
    DeletePayload {
        /// Payload keys to delete from the targeted points.
        delete_payload: DeletePayloadRequest,
    },
    /// `{ "update_vectors": … }` — replace point vectors.
    UpdateVectors {
        /// Vector sets to replace.
        update_vectors: UpdateVectorRequest,
    },
    /// `{ "delete_vectors": … }` — remove named vectors.
    DeleteVectors {
        /// Named vectors to remove.
        delete_vectors: DeleteVectorRequest,
    },
}

impl UpdateOperation {
    /// Human-readable operation name for executor responses.
    pub fn operation_name(&self) -> &'static str {
        match self {
            UpdateOperation::Upsert { .. } => "UPSERT",
            UpdateOperation::Delete { .. } => "DELETE",
            UpdateOperation::SetPayload { .. } => "UPDATE_PAYLOAD",
            UpdateOperation::Overwrite { .. } => "OVERWRITE_PAYLOAD",
            UpdateOperation::ClearPayload { .. } => "CLEAR_PAYLOAD",
            UpdateOperation::DeletePayload { .. } => "DELETE_PAYLOAD",
            UpdateOperation::UpdateVectors { .. } => "UPDATE_VECTOR",
            UpdateOperation::DeleteVectors { .. } => "DELETE_VECTOR",
        }
    }
}

/// Body for `POST /collections/{c}/points`: retrieve points by ID.
#[derive(Debug, Clone, Serialize)]
pub struct PointsRequest {
    /// Point IDs to fetch.
    pub ids: Vec<PlanPointId>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

// ── Scroll ─────────────────────────────────────────────────────

/// Body for `POST /collections/{c}/points/scroll` (keyset pagination).
#[derive(Debug, Clone, Serialize)]
pub struct ScrollRequest {
    /// Filter applied before paging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Point ID to resume from (`offset`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<PlanPointId>,
    /// Maximum points returned per page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Payload-key ordering instead of ID order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<OrderByQuery>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

// ── Mutations ──────────────────────────────────────────────────

/// Body for `PUT /collections/{c}/points` (upsert points).
#[derive(Debug, Clone, Serialize)]
pub struct UpsertRequest {
    /// Points to insert or overwrite.
    pub points: Vec<UpsertPointRequest>,
    /// `update_filter`: only matching points update (new points insert anyway).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_filter: Option<FilterExpression>,
    /// `update_mode`: insert_only / update_only / upsert (default when omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mode: Option<UpdateMode>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// OpenAPI `UpdateMode` for upserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// Only insert new points, do not update existing points.
    InsertOnly,
    /// Only update existing points, do not insert new points.
    UpdateOnly,
    /// Insert new points, update existing points (the default).
    Upsert,
}

impl UpsertRequest {
    /// Whether the request carries no points.
    ///
    /// Template planning (`plan_template`) skips whole-point placeholders
    /// (`VALUES :p`), so a param-only template yields an empty plan that must
    /// go through the point-splice path — never dispatch directly.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// One point in an upsert: ID plus optional vectors and payload.
#[derive(Debug, Clone, Serialize)]
pub struct UpsertPointRequest {
    /// Point ID.
    pub id: PlanPointId,
    /// Vectors to write, unnamed or keyed by vector name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<PlanPointVectors>,
    /// Payload object stored with the point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Body for `POST /collections/{c}/points/delete`.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteRequest {
    /// Explicit point IDs to delete (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to delete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// Body for `PUT /collections/{c}/points/vectors` (replace point vectors).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorRequest {
    /// Points with the vectors to replace.
    pub points: Vec<UpdateVectorPoint>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// One point in a vector update.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorPoint {
    /// Point ID.
    pub id: PlanPointId,
    /// Replacement vectors, unnamed or keyed by vector name.
    pub vector: PlanPointVectors,
}

/// Body for `POST /collections/{c}/points/payload` (set payload keys).
#[derive(Debug, Clone, Serialize)]
pub struct UpdatePayloadRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Payload keys to set on the selected points.
    pub payload: serde_json::Map<String, serde_json::Value>,
    /// Nested assignment path (OpenAPI `SetPayload.key`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// Body for `POST /collections/{c}/points/payload/clear` (drop all payload).
#[derive(Debug, Clone, Serialize)]
pub struct ClearPayloadRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// Body for `POST /collections/{c}/points/payload/delete` (remove keys).
#[derive(Debug, Clone, Serialize)]
pub struct DeletePayloadRequest {
    /// Payload keys to delete.
    pub keys: Vec<String>,
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// Body for `POST /collections/{c}/points/vectors/delete` (remove vectors).
#[derive(Debug, Clone, Serialize)]
pub struct DeleteVectorRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Named vectors to remove from the selected points.
    pub vector: Vec<String>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}

/// Body for `POST /collections/{c}/points/count`.
#[derive(Debug, Clone, Serialize)]
pub struct CountRequest {
    /// Filter narrowing the counted points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
    /// Exact count instead of a faster estimate when `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

/// Request payload for Qdrant's `/collections/{collection}/facet` endpoint or gRPC `Points.Facet`.
#[derive(Debug, Clone, Serialize)]
pub struct FacetRequest {
    /// Payload key to facet on.
    pub key: String,
    /// Maximum number of facet hits to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Filter expression narrowing points considered for faceting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Whether to return exact counts across shards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    /// Shard key for custom tenant routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
}
