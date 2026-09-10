//! Transport-neutral planning: AST → [`PlannedOperation`].
//!
//! Architecture:
//! - [`plan::plan`] is the sole statement → IR entry point.
//! - [`plan::to_rest_route`] is an **optional** REST projection (proxies / offline compile).
//! - Embeddings are owned by `qql-embed`, not this crate.

extern crate alloc;

/// Batch grouping, extraction, and validation for multi-statement execution.
pub mod batch;
mod bind;
/// DDL lowering: collection, index, and shard-key statements into plan requests.
pub mod ddl;
mod ddl_rest;
mod ddl_types;
/// Filter lowering into OpenAPI-shaped `Filter` condition structures.
pub mod filter;
mod filter_types;
mod formula_types;
/// Payload index request types (`CREATE INDEX`).
mod index_types;
/// Mutation lowering: upsert, delete, and payload/vector updates into wire bodies.
pub mod mutation;
mod mutation_types;
mod params;
pub mod plan;
mod prefetch;
/// Query lowering: `QUERY` statements into `/points/query` request bodies.
pub mod query;
mod query_types;
mod rerank;
/// Optional REST route projection and the offline `compile_statement` entry point.
pub mod routing;
pub mod semantic;
/// Wire and plan-IR request types shared by the REST projection and gRPC conversion.
pub mod types;
mod validate;

pub use batch::BatchGrouper;
pub use formula_types::{FormulaDefault, PlanDecayKind, PlanFormula};
pub use plan::{
    BatchFamily, BatchKey, PlannedOperation, RestProjectionError, batch_item_error,
    build_query_batch, build_update_batch, ensure_no_unbound_params, parse_and_plan, plan,
    plan_template, statement_batch_key, to_rest_route, try_route, verify_batch_cardinality,
};
pub use routing::{CompiledStatement, compile_statement};
pub use semantic::{
    PlanFacetValue, PlanGroupId, PlanPointId, PlanPointVectors, PlanQueryInput, PlanShardKey,
    PlanVectorStruct, PlanVectorValue,
};
pub use types::*;
