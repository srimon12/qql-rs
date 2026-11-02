//! Qdrant 1.20 request shapes that QQL cannot spell yet.
//!
//! Qdrant 1.20 added BM25 `text` queries, `scoring` / `nested` on payload
//! indexes, the `code` tokenizer, `substring` match, the `turbo8` /
//! `turbo16` datatypes, `bits8` TurboQuant, and the id-tracker memory tier.
//! BM25 `text` queries (`QUERY BM25`), text-index `scoring = 'bm25'`, and
//! `tokenizer = 'code'` now convert (QQL 1.9). For the rest, until the QQL
//! grammar and AST grow them, `qql convert` fails closed on them with one
//! recognizable diagnostic instead of a generic "unknown field". When core support lands, the matching arm here is
//! replaced by a real decode and its `pending_*` test flips to a round-trip.

use crate::ConvertError;
use crate::json::invalid;

/// Stable prefix of every pending-1.20 diagnostic (asserted by tests and
/// greppable in `qql convert` output).
pub(crate) const PENDING_PREFIX: &str = "Qdrant 1.20";

/// Datatype names introduced in Qdrant 1.20.
pub(crate) const DATATYPES_1_20: [&str; 2] = ["turbo8", "turbo16"];

/// TurboQuant bit-size labels introduced in Qdrant 1.20.
pub(crate) const TURBO_BITS_1_20: [&str; 1] = ["bits8"];

/// Fail closed on a Qdrant 1.20 feature that QQL cannot express yet.
pub(crate) fn pending(path: impl Into<String>, feature: &str) -> ConvertError {
    invalid(
        path,
        format!(
            "{PENDING_PREFIX} {feature} is not expressible in QQL yet; \
             convert fails closed until QQL grammar support lands"
        ),
    )
}

/// True when `name` (case-insensitive) is in `table`.
pub(crate) fn is_new(table: &[&str], name: &str) -> bool {
    table.iter().any(|t| t.eq_ignore_ascii_case(name))
}
