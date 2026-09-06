//! Transport-neutral planning: AST → [`PlannedOperation`].
//!
//! Architecture:
//! - [`plan::plan`] is the sole statement → IR entry point.
//! - [`plan::to_rest_route`] is an **optional** REST projection (proxies / offline compile).
//! - Embeddings are owned by `qql-embed`, not this crate.

extern crate alloc;

/// DDL lowering: collection, index, and shard-key statements into plan requests.
pub mod ddl;
/// Filter lowering into OpenAPI-shaped `Filter` condition structures.
pub mod filter;
/// Mutation lowering: upsert, delete, and payload/vector updates into wire bodies.
pub mod mutation;
pub mod plan;
/// Query lowering: `QUERY` statements into `/points/query` request bodies.
pub mod query;
/// Optional REST route projection and the offline `compile_statement` entry point.
pub mod routing;
pub mod semantic;
/// Wire and plan-IR request types shared by the REST projection and gRPC conversion.
pub mod types;

pub use plan::{
    BatchFamily, BatchKey, PlannedOperation, RestProjectionError, parse_and_plan, plan,
    statement_batch_key, to_rest_route, try_route,
};
pub use routing::{CompiledStatement, compile_statement};
pub use semantic::{PlanFormula, PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue};
pub use types::*;
