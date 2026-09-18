//! Autofix engine for `qql lint` (`--fix`).
//!
//! Split from `lint.rs` (size hygiene): excision of redundant
//! `WITH PAYLOAD true` runs and duplicate `WAIT` clauses plus the canonical
//! reformat fixpoint. Token scanning (`lex_kinds`, run finders, and the
//! redundant-payload census) stays in `lint`; this module calls back into
//! those `pub(crate)` helpers. Behavior is unchanged.

use super::convert::source_is_canonical;
use super::lint::{count_redundant_payload, lex_kinds, payload_true_runs, wait_runs_around};
use qql_core::parser::Parser;

/// Retreat an excision start over spaces/tabs (never newlines) so the fix
/// does not leave double spaces behind.
fn excise_start(source: &str, s: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = s.min(bytes.len());
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    i
}

pub(crate) fn apply_fixes(source: &str) -> (String, usize) {
    // Fixpoint: excision can unblock formatting and formatting can unblock
    // excision (e.g. a comment between WITH and PAYLOAD that fmt relocates).
    // Excision strictly shrinks the source and fmt is idempotent, so this
    // converges; the cap is a backstop.
    let mut current = source.to_string();
    let mut fixes = 0;
    for _ in 0..4 {
        let (next, n) = apply_fixes_once(&current);
        fixes += n;
        if next == current {
            break;
        }
        current = next;
    }
    (current, fixes)
}

fn apply_fixes_once(source: &str) -> (String, usize) {
    let recovered = Parser::parse_all_recovering(source);
    let Some(toks) = lex_kinds(source) else {
        return (source.to_string(), 0);
    };
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    // Redundant payload: excise the clause runs inside statements whose AST
    // claims them (never inside string literals or comments — the lexer
    // guarantees token spans are real code).
    for (stmt, span) in &recovered.statements {
        let expected = count_redundant_payload(stmt);
        if expected == 0 {
            continue;
        }
        for (rs, re) in payload_true_runs(source, &toks, span.start, span.end)
            .into_iter()
            .take(expected)
        {
            ranges.push((excise_start(source, rs), re));
        }
    }

    // Duplicate WAIT: the statement failed to parse, so scope by the parser's
    // error span instead — the semicolon-delimited segment around it — and
    // keep the trailing clause, matching the diagnostic hint.
    for err in &recovered.errors {
        if err.code != "QQL-PARSE-DUPLICATE-CLAUSE" {
            continue;
        }
        let err_pos = err.span.map(|s| s.start).unwrap_or(0);
        let runs = wait_runs_around(source, &toks, err_pos);
        if runs.len() > 1 {
            for (rs, re) in runs.iter().take(runs.len() - 1) {
                ranges.push((excise_start(source, *rs), *re));
            }
        }
    }

    ranges.sort_unstable();
    ranges.dedup();
    let mut fixes = ranges.len();
    let mut out = source.to_string();
    for (rs, re) in ranges.into_iter().rev() {
        if rs < re && re <= out.len() && out.is_char_boundary(rs) && out.is_char_boundary(re) {
            out.replace_range(rs..re, "");
        } else {
            fixes = fixes.saturating_sub(1);
        }
    }

    if let Ok(formatted) = qql_core::fmt::format(&out) {
        if !source_is_canonical(&out, &formatted) {
            fixes += 1;
        }
        (formatted, fixes)
    } else {
        (out, fixes)
    }
}
