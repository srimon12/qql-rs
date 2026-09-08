//! gRPC status → [`QqlError`] mapping (shared by all grpc submodules).

use qql_core::error::QqlError;

/// Convert a [`tonic::Status`] into a structured `QqlError` that preserves
/// the gRPC status code and operation name as machine-readable context.
///
/// The error code is `QQL-GRPC` and the message includes the original status
/// message. The gRPC status code is attached via `.with_field("grpc_code", ...)`.
/// `request_id` is the client-generated correlation id recorded by the
/// interceptor for the outgoing RPC, so a failure can be matched against
/// Qdrant's log lines (mirrors the REST `x-request-id` echo). An empty id
/// (no RPC reached the interceptor) is omitted from both message and fields.
pub(crate) fn grpc_error(operation: &str, status: tonic::Status, request_id: &str) -> QqlError {
    let message = if request_id.is_empty() {
        format!("{operation}: {status}")
    } else {
        format!("{operation}: {status} (request id: {request_id})")
    };
    let code = match status.code() {
        tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => "QQL-BACKEND-AUTH",
        tonic::Code::NotFound => "QQL-BACKEND-COLLECTION-NOT-FOUND",
        _ => {
            let msg = status.message().to_ascii_lowercase();
            if msg.contains("no appropriate index")
                || msg.contains("index not found")
                || msg.contains("index is not ready")
                || msg.contains("indexing")
            {
                "QQL-BACKEND-INDEX-NOT-READY"
            } else if msg.contains("dimension")
                || msg.contains("vector size")
                || msg.contains("dimensions")
            {
                "QQL-BACKEND-DIMENSION-MISMATCH"
            } else if msg.contains("strict-mode")
                || msg.contains("strict mode")
                || msg.contains("quota exceeded")
            {
                "QQL-BACKEND-STRICT-MODE"
            } else {
                "QQL-GRPC"
            }
        }
    };
    let error = QqlError::backend(code, message, None)
        .with_field("grpc_code", format!("{}", status.code() as i32))
        .with_field("operation", operation.to_string());
    if request_id.is_empty() {
        error
    } else {
        error.with_field("request_id", request_id.to_string())
    }
}
