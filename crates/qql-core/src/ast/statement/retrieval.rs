//! Typed AST for retrieval statements (SCROLL, COUNT, FACET).

use super::query::QueryCollection;
use super::types::*;
use crate::ast::{FilterExpr, Value};
use alloc::string::String;

/// `ORDER BY` tail of a `SCROLL` statement (OpenAPI `ScrollRequest.order_by`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScrollOrderBy {
    /// Payload field to sort on.
    pub field: String,
    /// Sort direction (`ASC` default).
    pub direction: OrderDirection,
    /// Optional paging origin: resume ordering from this payload value
    /// (OpenAPI `OrderBy.start_from`: integer, float, or datetime string).
    pub start_from: Option<Value>,
}

/// `SCROLL FROM <collection> … LIMIT n` — cursor-based point iteration.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScrollStmt {
    /// Collection to scroll.
    pub collection: String,
    /// Maximum number of points per page.
    pub limit: u64,
    /// Optional `WHERE` filter.
    pub filter: Option<Box<FilterExpr>>,
    /// `AFTER` cursor — resume scrolling after this point ID.
    pub after: Option<PointId>,
    /// Optional `ORDER BY` payload ordering (OpenAPI `order_by`).
    pub order_by: Option<ScrollOrderBy>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// Optional `WITH PAYLOAD` selector. Defaults to all payload when `None`.
    pub with_payload: Option<PayloadSelector>,
    /// Optional `WITH VECTOR` selector. Defaults to no vectors when `None`.
    pub with_vector: Option<VectorSelector>,
    /// Optional limit parameter placeholder (`:limit` or `?`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_param: Option<String>,
    /// Source span of the limit parameter placeholder, if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_span: Option<crate::error::Span>,
}

/// `COUNT FROM <collection> [WHERE …]` statement.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CountStmt {
    /// Collection to count (explicit or inherited).
    pub collection: QueryCollection,
    /// Optional `WHERE` filter.
    pub filter: Option<Box<FilterExpr>>,
    /// `SHARD '<key>'` routing key.
    pub shard_key: Option<super::ShardKey>,
    /// `WITH (exact = …)` — require exact counts.
    pub exact: Option<bool>,
}

/// In-database categorical facet aggregation statement (`FACET <key> FROM <collection>`).
///
/// Compiles to Qdrant's `/collections/{collection}/facet` endpoint (REST) or `Points.Facet` (gRPC), returning hit counts
/// per unique value for a payload field without retrieving full point records.
///
/// # Supported clauses
/// - `WHERE`: Optional filter restricting candidate points.
/// - `LIMIT`: Maximum number of distinct facet values to return.
/// - `EXACT`: Whether to compute exact distributed counts across shards.
/// - `SHARD`: Target shard key for tenant-partitioned collections.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FacetStmt {
    /// Payload field name to aggregate values for.
    pub key: String,
    /// Target collection.
    pub collection: QueryCollection,
    /// Optional point filter scoping the aggregation.
    pub filter: Option<Box<FilterExpr>>,
    /// Maximum number of unique facet hits to return.
    pub limit: Option<u64>,
    /// Whether to compute exact distributed counts across shards.
    pub exact: Option<bool>,
    /// Optional shard key partition routing.
    pub shard_key: Option<super::ShardKey>,
    /// Optional limit parameter placeholder (`:limit` or `?`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_param: Option<String>,
    /// Source span of the limit parameter placeholder, if unbound.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub limit_span: Option<crate::error::Span>,
}
