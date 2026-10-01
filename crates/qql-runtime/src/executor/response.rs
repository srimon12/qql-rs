//! Shared response surface, re-exported from [`qql_plan::surface::response`].
//!
//! The closed `ExecData` family, score normalization, and response envelopes
//! live in `qql-plan::surface` so the runtime and WASM hosts share one shape.
//! This module keeps the historical `crate::executor::*` paths stable.

pub use qql_plan::surface::response::*;
