//! Typed version gate for Qdrant >= 1.20 text features (QQL 1.9).
//!
//! QQL has no server-version probe, so a statement that lowers to a 1.20-only
//! wire shape (`QUERY BM25` → `{"text": …}`, text index `scoring` / `code`
//! tokenizer) is sent as-is. When an older server rejects that shape with a
//! decode-style error, this maps the raw backend error to the typed
//! `QQL-BACKEND-VERSION` code (original error chained as the source) instead
//! of surfacing an opaque Qdrant 400.
//!
//! Known gap: over gRPC an older server ignores unknown proto fields, so text
//! index `scoring` is dropped silently there (no error to map).

use qql_core::error::{ErrorKind, QqlError};

/// Backend error fragments an older Qdrant emits for an unknown query
/// variant / index option (REST serde errors, gRPC conversion errors).
const DECODE_MARKERS: &[&str] = &[
    "unknown variant",
    "did not match any variant",
    "untagged enum",
    "format error in json body",
    "json deserialize error",
    "unknown field",
    "unknown tokenizer",
    "invalid tokenizer",
    "tokenizer type",
    "query variant",
    "unknown query",
    "query is missing",
    "missing query",
];

/// Map an older server's decode error for a 1.20-only feature to
/// `QQL-BACKEND-VERSION`. Any other error passes through unchanged.
pub(crate) fn gate_qdrant_1_20(feature: Option<&'static str>, error: QqlError) -> QqlError {
    let Some(feature) = feature else {
        return error;
    };
    if error.kind != ErrorKind::Backend {
        return error;
    }
    let lower = error.message.to_ascii_lowercase();
    if !DECODE_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return error;
    }
    let mut gated = QqlError::backend(
        "QQL-BACKEND-VERSION",
        format!(
            "{feature} requires Qdrant >= {min}; the server rejected the request shape, \
             which usually means an older Qdrant. Upgrade Qdrant, or use client-side \
             sparse BM25 (QUERY TEXT '…' USING <sparse vector>). Server said: {message}",
            min = qql_plan::QDRANT_TEXT_BM25_MIN_VERSION,
            message = error.message,
        ),
        None,
    )
    .with_field("required_qdrant", qql_plan::QDRANT_TEXT_BM25_MIN_VERSION)
    .with_field("feature", feature);
    for field in &error.fields {
        gated = gated.with_field(field.key.clone(), field.value.clone());
    }
    gated.caused_by(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_server_decode_error_maps_to_typed_version_error() {
        let raw = QqlError::backend(
            "QQL-BACKEND",
            "Qdrant returned 400 Bad Request: {\"status\":{\"error\":\"Format error in JSON body: \
             data did not match any variant of untagged enum QueryInterface\"}}",
            None,
        )
        .with_status(400);
        let gated = gate_qdrant_1_20(Some("QUERY BM25"), raw.clone());
        assert_eq!(gated.code, "QQL-BACKEND-VERSION");
        assert!(
            gated.message.contains("Qdrant >= 1.20"),
            "{}",
            gated.message
        );
        assert!(gated.message.contains("QUERY BM25"), "{}", gated.message);
        assert_eq!(gated.source.as_deref(), Some(&raw));

        let grpc = QqlError::backend(
            "QQL-GRPC",
            "create_field_index: status: InvalidArgument, message: \"Unknown tokenizer type: 5\"",
            None,
        );
        assert_eq!(
            gate_qdrant_1_20(Some("text index tokenizer = 'code'"), grpc).code,
            "QQL-BACKEND-VERSION"
        );
    }

    #[test]
    fn unrelated_errors_and_plain_statements_pass_through() {
        let not_found = QqlError::backend(
            "QQL-BACKEND-COLLECTION-NOT-FOUND",
            "Qdrant returned 404 Not Found: collection books not found",
            None,
        );
        assert_eq!(
            gate_qdrant_1_20(Some("QUERY BM25"), not_found.clone()),
            not_found
        );
        let decode = QqlError::backend("QQL-BACKEND", "unknown variant `text`", None);
        assert_eq!(gate_qdrant_1_20(None, decode.clone()), decode);
        let transport = QqlError::transport("QQL-TRANSPORT", "unknown variant", None);
        assert_eq!(
            gate_qdrant_1_20(Some("QUERY BM25"), transport.clone()),
            transport
        );
    }
}
