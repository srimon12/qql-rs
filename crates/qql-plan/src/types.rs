pub use crate::semantic::{
    PlanFormula, PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue,
};
pub use qql_core::ast::{MemoryPlacement, VectorDatatype};

// ── Method ──────────────────────────────────────────────────────

/// HTTP verb of a projected REST route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP `GET`.
    Get,
    /// HTTP `POST`.
    Post,
    /// HTTP `PUT`.
    Put,
    /// HTTP `PATCH`.
    Patch,
    /// HTTP `DELETE`.
    Delete,
}

impl Method {
    /// Uppercase verb string, e.g. `"POST"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

pub use crate::ddl_types::*;
pub use crate::filter_types::*;
pub use crate::mutation_types::*;
pub use crate::query_types::*;
