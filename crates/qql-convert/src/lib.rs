//! Qdrant REST JSON to QQL statement converter.
//!
//! Two input shapes are accepted:
//!
//! 1. **Wrapped request** — `{"method": ..., "path": ..., "body": ...}`; the
//!    collection is derived from the path and any caller-supplied collection
//!    is ignored.
//! 2. **Bare body** — raw Qdrant REST JSON without path context; the caller
//!    supplies the collection via [`json_to_qql_with_collection`]
//!    ([`json_to_qql`] falls back to `"unknown"`).
//!
//! Conversion is deterministic: the same JSON always yields the same QQL.
//! Every emitted statement is valid QQL — unsupported geo predicates fail
//! with [`ConvertError::GeoUnsupported`] instead of emitting placeholders.

mod converter;
mod decoder;
mod detect;
mod filter;
mod formatters;
mod formulas;
mod operations;
mod rest_types;
mod sanitize;

pub use converter::{json_to_qql, json_to_qql_with_collection};
use std::fmt;

/// Typed conversion failure.
///
/// Returned by [`json_to_qql`] and [`json_to_qql_with_collection`].
#[derive(Debug, PartialEq, Eq)]
pub enum ConvertError {
    /// The input is not valid JSON. Holds the underlying parse message.
    InvalidJson(String),
    /// A wrapped request targets an endpoint with no QQL mapping.
    /// Holds `"METHOD path"`.
    UnsupportedEndpoint(String),
    /// A bare body matched no known operation shape.
    UndetectableOperation,
    /// A structurally recognized payload has invalid content.
    /// Holds a static reason (e.g. `"no points in upsert payload"`).
    InvalidPayload(&'static str),
    /// A geo predicate (`geo_bounding_box`, `geo_radius`, `geo_polygon`)
    /// has no QQL representation. Holds the predicate name.
    GeoUnsupported(&'static str),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(e) => write!(f, "invalid JSON: {e}"),
            Self::UnsupportedEndpoint(ep) => write!(f, "unsupported endpoint: {ep}"),
            Self::UndetectableOperation => {
                write!(f, "cannot detect operation from JSON structure")
            }
            Self::InvalidPayload(reason) => write!(f, "{reason}"),
            Self::GeoUnsupported(pred) => {
                write!(f, "geo predicate '{pred}' has no QQL representation")
            }
        }
    }
}

impl std::error::Error for ConvertError {}
