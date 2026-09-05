//! gRPC status → [`QqlError`] mapping (shared by all grpc submodules).

use qql_core::error::QqlError;

/// Convert a [`tonic::Status`] into a structured `QqlError` that preserves
/// the gRPC status code and operation name as machine-readable context.
///
/// The error code is `QQL-GRPC` and the message includes the original status
/// message. The gRPC status code is attached via `.with_field("grpc_code", ...)`.
pub(crate) fn grpc_error(operation: &str, status: tonic::Status) -> QqlError {
    QqlError::backend("QQL-GRPC", format!("{operation}: {status}"), None)
        .with_field("grpc_code", format!("{}", status.code() as i32))
        .with_field("operation", operation.to_string())
}
