//! Regression tests for the shared canonical-source check behind
//! `qql fmt --check` and `qql check` stage 1.
//!
//! The bug this pins: `fmt::format` always ends with a newline, so comparing
//! a trimmed input against the untrimmed output reported every file as
//! unformatted.

use crate::commands::source_is_canonical;

const CANONICAL: &str = "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n";

#[test]
fn canonical_with_or_without_trailing_newline_passes() {
    assert!(source_is_canonical(CANONICAL, CANONICAL));
    assert!(source_is_canonical(
        "QUERY [0.1, 0.2] FROM docs LIMIT 5;",
        CANONICAL
    ));
    assert!(source_is_canonical(
        "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n\n",
        CANONICAL
    ));
}

#[test]
fn non_canonical_source_fails() {
    assert!(!source_is_canonical(
        "query [0.1, 0.2] from docs limit 5;",
        CANONICAL
    ));
    assert!(!source_is_canonical(
        "QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5;",
        CANONICAL
    ));
}
