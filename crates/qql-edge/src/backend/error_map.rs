//! Stable mapping of `qdrant-edge` engine failures onto `QQL-EDGE-*` codes.
//!
//! `qdrant-edge` exposes exactly one public error type,
//! [`qdrant_edge::OperationError`]: its internal error enums (blobstore,
//! universal I/O, WAL, quota, grouping, quantization, threading, …) are private
//! and the crate already folds them into an `OperationError` variant — mostly
//! `ServiceError` with the original `Display` text preserved (`Blobstore
//! error: …`, `IO Error: …`, `failed to load segment …`), `Cancelled` for
//! cancelled flushes/encoding, and `FileNotFound` for structured not-found
//! errors. This module therefore classifies by `OperationError` variant — the
//! public contract — and forwards the crate's own error text so the underlying
//! cause stays visible. No private error enum gets a code of its own.

use qdrant_edge::OperationError;
use qql_core::error::{ErrorKind, QqlError};

/// The engine operation a failure came from. Used in the message and attached
/// as the structured `operation` field so a code can be traced to its call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeOp {
    /// `EdgeShard::load` — opening an existing collection directory.
    Load,
    /// `EdgeShard::new` — creating a collection directory.
    Create,
    /// `EdgeShard::query`.
    Query,
    /// `EdgeShard::retrieve`.
    Retrieve,
    /// `EdgeShard::scroll`.
    Scroll,
    /// `EdgeShard::count`.
    Count,
    /// `EdgeShard::facet`.
    Facet,
    /// `EdgeShard::update` with an upsert.
    Upsert,
    /// `EdgeShard::update` with a point delete.
    Delete,
    /// `EdgeShard::update` with a payload set.
    UpdatePayload,
    /// `EdgeShard::update` with a payload clear.
    ClearPayload,
    /// `EdgeShard::update` with a payload-key delete.
    DeletePayload,
    /// `EdgeShard::update` with a vector update.
    UpdateVectors,
    /// `EdgeShard::update` with a vector delete.
    DeleteVectors,
    /// `EdgeShard::update` with a payload-index create.
    CreateIndex,
    /// `EdgeShard::update` with a payload-index delete.
    DropIndex,
    /// `EdgeShard::optimize`.
    Optimize,
    /// `EdgeShard::info`.
    Info,
    /// `EdgeShard::unpack_snapshot` / `EdgeShard::load` during snapshot seeding
    /// (`qql edge bootstrap`).
    Snapshot,
    /// `EdgeShard::set_hnsw_config` / `set_vector_hnsw_config` /
    /// `set_optimizers_config` (`ALTER COLLECTION`).
    AlterCollection,
    /// Formula lowering (`FormulaInternal` → parsed formula).
    Formula,
}

impl EdgeOp {
    /// Stable snake-case label used in messages and the `operation` field.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Create => "create",
            Self::Query => "query",
            Self::Retrieve => "retrieve",
            Self::Scroll => "scroll",
            Self::Count => "count",
            Self::Facet => "facet",
            Self::Upsert => "upsert",
            Self::Delete => "delete",
            Self::UpdatePayload => "update_payload",
            Self::ClearPayload => "clear_payload",
            Self::DeletePayload => "delete_payload",
            Self::UpdateVectors => "update_vectors",
            Self::DeleteVectors => "delete_vectors",
            Self::CreateIndex => "create_index",
            Self::DropIndex => "drop_index",
            Self::Optimize => "optimize",
            Self::Info => "info",
            Self::Snapshot => "snapshot",
            Self::AlterCollection => "alter_collection",
            Self::Formula => "formula",
        }
    }
}

/// Map a `qdrant-edge` engine failure to a stable, variant-precise error.
///
/// The `OperationError` variant is preserved as the code selector; the
/// crate's own `Display` text is kept verbatim in the message so the
/// underlying cause (I/O error, corrupt segment, failed WAL flush, …) is not
/// lost. `collection` is attached as structured context when known.
///
/// Kinds mirror the remote backends' split: request-shaped failures (bad
/// input, wrong types, unknown points/vectors) are
/// [`ErrorKind::Validation`], engine/storage failures are
/// [`ErrorKind::Backend`]. The codes stay `QQL-EDGE-*` in every case.
pub(crate) fn edge_err(op: EdgeOp, collection: Option<&str>, error: OperationError) -> QqlError {
    let (code, kind) = match &error {
        OperationError::WrongVectorDimension { .. } => {
            ("QQL-EDGE-DIMENSION", ErrorKind::Validation)
        }
        OperationError::MalformedVectorBlob { .. }
        | OperationError::ValidationError { .. }
        | OperationError::TypeInferenceError { .. }
        | OperationError::WrongSparse
        | OperationError::WrongMulti
        | OperationError::NonFiniteNumber { .. } => ("QQL-EDGE-BAD-INPUT", ErrorKind::Validation),
        OperationError::TypeError { .. } | OperationError::VariableTypeError { .. } => {
            ("QQL-EDGE-TYPE", ErrorKind::Validation)
        }
        OperationError::VectorNameNotExists { .. } => {
            ("QQL-EDGE-VECTOR-NAME", ErrorKind::Validation)
        }
        OperationError::PointIdError { .. } => ("QQL-EDGE-POINT-NOT-FOUND", ErrorKind::Validation),
        // `ServiceError` is the crate's catch-all for I/O, lock acquisition,
        // and internal invariant failures; `OutOfAppendableCapacity` is a
        // segment-size exhaustion surfaced through the same path.
        OperationError::ServiceError { .. } | OperationError::OutOfAppendableCapacity { .. } => {
            ("QQL-EDGE-STORAGE", ErrorKind::Backend)
        }
        OperationError::InconsistentStorage { .. } => ("QQL-EDGE-CORRUPT", ErrorKind::Backend),
        OperationError::FileNotFound { .. } => ("QQL-EDGE-FILE-NOT-FOUND", ErrorKind::Backend),
        OperationError::OutOfMemory { .. } => ("QQL-EDGE-OOM", ErrorKind::Backend),
        OperationError::Cancelled { .. } => ("QQL-EDGE-CANCELLED", ErrorKind::Backend),
        OperationError::Timeout { .. } => ("QQL-EDGE-TIMEOUT", ErrorKind::Backend),
        OperationError::MissingRangeIndexForOrderBy { .. }
        | OperationError::MissingMapIndexForFacet { .. } => {
            ("QQL-EDGE-MISSING-INDEX", ErrorKind::Backend)
        }
    };
    let error = build_error(
        kind,
        code,
        format!("qdrant-edge {} failed: {error}", op.label()),
    )
    .with_field("operation", op.label());
    match collection {
        Some(collection) => error.with_collection(collection.to_string()),
        None => error,
    }
}

/// Build a `QqlError` of `kind` with `code` and `message`.
fn build_error(kind: ErrorKind, code: &'static str, message: String) -> QqlError {
    match kind {
        ErrorKind::Validation => QqlError::validation(code, message, None),
        ErrorKind::Backend => QqlError::backend(code, message, None),
        _ => QqlError::execution(code, message, None),
    }
}

/// A client-side input rejection at the edge boundary, reported with the same
/// `QQL-EDGE-BAD-INPUT` code the engine's validation failures use.
///
/// The engine never ran, so the message is the caller-facing reason only; the
/// `operation` field still records which conversion rejected the input.
pub(crate) fn edge_input_err(
    op: EdgeOp,
    collection: Option<&str>,
    message: impl Into<String>,
) -> QqlError {
    let error = QqlError::validation("QQL-EDGE-BAD-INPUT", message.into(), None)
        .with_field("operation", op.label());
    match collection {
        Some(collection) => error.with_collection(collection.to_string()),
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn assert_mapping(error: OperationError, expected: &str, expected_kind: ErrorKind) {
        let mapped = edge_err(EdgeOp::Query, Some("docs"), error);
        assert_eq!(mapped.code, expected, "wrong code: {mapped}");
        assert_eq!(mapped.kind, expected_kind, "wrong kind for {expected}");
        assert_eq!(mapped.field("collection"), Some("docs"));
        assert_eq!(mapped.field("operation"), Some("query"));
        assert!(
            mapped.message.contains("qdrant-edge query failed:"),
            "message lacks operation context: {}",
            mapped.message
        );
    }

    /// Every `OperationError` variant maps to its own category; this is the
    /// stable public contract, pinned so a dependency bump cannot silently
    /// reclassify engine failures (code or kind).
    #[test]
    fn operation_error_variants_map_to_stable_codes() {
        assert_mapping(
            OperationError::WrongVectorDimension {
                expected_dim: 3,
                received_dim: 2,
            },
            "QQL-EDGE-DIMENSION",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::MalformedVectorBlob {
                description: "bad blob".into(),
            },
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::VectorNameNotExists {
                received_name: "dense".into(),
            },
            "QQL-EDGE-VECTOR-NAME",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::PointIdError {
                missed_point_id: qdrant_edge::PointId::NumId(7),
            },
            "QQL-EDGE-POINT-NOT-FOUND",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::TypeError {
                field_name: "city".parse().unwrap(),
                expected_type: "keyword".into(),
            },
            "QQL-EDGE-TYPE",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::TypeInferenceError {
                field_name: "city".parse().unwrap(),
            },
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::service_error("IO Error: Permission denied"),
            "QQL-EDGE-STORAGE",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::inconsistent_storage("segment 0 is missing its index"),
            "QQL-EDGE-CORRUPT",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::FileNotFound {
                path: PathBuf::from("/data/docs/segments/0/vector.dat"),
            },
            "QQL-EDGE-FILE-NOT-FOUND",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::OutOfMemory {
                description: "IO Error: Cannot allocate memory".into(),
                free: 0,
            },
            "QQL-EDGE-OOM",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::cancelled("process cancelled by service"),
            "QQL-EDGE-CANCELLED",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::timeout(std::time::Duration::from_secs(5), "retrieve"),
            "QQL-EDGE-TIMEOUT",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::validation_error("prefetches depth 65 exceeds max depth 64"),
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::WrongSparse,
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::WrongMulti,
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::MissingRangeIndexForOrderBy {
                key: "price".into(),
            },
            "QQL-EDGE-MISSING-INDEX",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::MissingMapIndexForFacet { key: "city".into() },
            "QQL-EDGE-MISSING-INDEX",
            ErrorKind::Backend,
        );
        assert_mapping(
            OperationError::VariableTypeError {
                field_name: "price".parse().unwrap(),
                expected_type: "number".into(),
                description: "Value is not a number".into(),
            },
            "QQL-EDGE-TYPE",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::NonFiniteNumber {
                expression: "$score / price".into(),
            },
            "QQL-EDGE-BAD-INPUT",
            ErrorKind::Validation,
        );
        assert_mapping(
            OperationError::OutOfAppendableCapacity {
                max_segment_size_bytes: 1024,
            },
            "QQL-EDGE-STORAGE",
            ErrorKind::Backend,
        );
    }

    /// The crate's own cause text survives into the message.
    #[test]
    fn service_error_keeps_crate_description() {
        let mapped = edge_err(
            EdgeOp::Load,
            Some("docs"),
            OperationError::service_error("failed to load segment /data/docs/segments/0: IO Error"),
        );
        assert_eq!(mapped.code, "QQL-EDGE-STORAGE");
        assert_eq!(mapped.kind, ErrorKind::Backend);
        assert!(
            mapped
                .message
                .contains("failed to load segment /data/docs/segments/0"),
            "cause text lost: {}",
            mapped.message
        );
    }

    /// `edge_input_err` shares the engine's bad-input code and kind, names the
    /// op in the structured field, and never claims the engine ran.
    #[test]
    fn input_errors_use_bad_input_code() {
        let mapped = edge_input_err(EdgeOp::Facet, Some("docs"), "limit exceeds platform usize");
        assert_eq!(mapped.code, "QQL-EDGE-BAD-INPUT");
        assert_eq!(mapped.kind, ErrorKind::Validation);
        assert_eq!(mapped.message, "limit exceeds platform usize");
        assert_eq!(mapped.field("collection"), Some("docs"));
        assert_eq!(mapped.field("operation"), Some("facet"));

        let no_collection = edge_input_err(EdgeOp::Formula, None, "invalid formula");
        assert_eq!(no_collection.field("collection"), None);
        assert_eq!(no_collection.field("operation"), Some("formula"));
    }

    #[test]
    fn labels_are_stable_and_unique() {
        let ops = [
            EdgeOp::Load,
            EdgeOp::Create,
            EdgeOp::Query,
            EdgeOp::Retrieve,
            EdgeOp::Scroll,
            EdgeOp::Count,
            EdgeOp::Facet,
            EdgeOp::Upsert,
            EdgeOp::Delete,
            EdgeOp::UpdatePayload,
            EdgeOp::ClearPayload,
            EdgeOp::DeletePayload,
            EdgeOp::UpdateVectors,
            EdgeOp::DeleteVectors,
            EdgeOp::CreateIndex,
            EdgeOp::DropIndex,
            EdgeOp::Optimize,
            EdgeOp::Info,
            EdgeOp::Snapshot,
            EdgeOp::AlterCollection,
            EdgeOp::Formula,
        ];
        let mut labels = std::collections::BTreeSet::new();
        for op in ops {
            assert!(labels.insert(op.label()), "duplicate label {}", op.label());
        }
    }
}
