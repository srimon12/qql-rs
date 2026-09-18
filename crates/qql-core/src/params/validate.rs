//! Validation for unbound parameters in AST statements.
//!
//! Thin folds over the single census traversal in `validate_collect`: each
//! entry point runs one walk and extracts its answer. No walker logic lives
//! here.

use crate::ast::statement::Stmt;
use crate::error::QqlError;

pub use super::validate_collect::{collect_statement_params, stmt_has_point_params};

/// Verify that a statement contains no unbound parameters, without cloning the AST.
pub fn validate_no_unbound_params(stmt: &Stmt) -> Result<(), QqlError> {
    let census = super::validate_collect::run_census(stmt);
    match census.full_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Verify that a statement contains no unbound scalar parameters, allowing
/// vector parameter placeholders (`VectorValue::Param`, `PointVectors::Param`,
/// `QueryInput::Param`). Returns `Ok(true)` if vector parameters exist.
pub fn validate_no_unbound_scalar_params(stmt: &Stmt) -> Result<bool, QqlError> {
    let census = super::validate_collect::run_census(stmt);
    match census.scalar_err {
        Some(err) => Err(err),
        None => Ok(census.has_vec_params),
    }
}
