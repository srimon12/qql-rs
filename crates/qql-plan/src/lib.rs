//! Transport-neutral planning: AST → [`PlannedOperation`].
//!
//! Architecture:
//! - [`plan::plan`] is the sole statement → IR entry point.
//! - [`plan::to_rest_route`] is an **optional** REST projection (proxies / offline compile).
//! - Embeddings are owned by `qql-embed`, not this crate.

extern crate alloc;

pub mod ddl;
pub mod filter;
pub mod mutation;
pub mod plan;
pub mod query;
pub mod routing;
pub mod semantic;
pub mod types;

pub use plan::{
    parse_and_plan, plan, statement_batch_key, to_rest_route, try_route, BatchFamily, BatchKey,
    PlannedOperation, RestProjectionError,
};
pub use routing::{compile_statement, CompiledStatement};
pub use semantic::{PlanFormula, PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue};
pub use types::*;
