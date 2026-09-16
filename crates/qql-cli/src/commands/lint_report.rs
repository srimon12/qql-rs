//! Human-readable lint report rendering for `qql lint`.
//!
//! Split from `lint.rs` (size hygiene): the determinant codeframe renderer
//! and the colored stderr report printer. The report types live in `lint`.
//! Behavior is unchanged.

use super::lint::FileLintReport;

fn render_codeframe(source: &str, line_no: usize, col_no: usize, span_len: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line_no == 0 || line_no > lines.len() {
        return String::new();
    }
    let target_line = lines[line_no - 1];
    let gutter = format!("{:>4} | ", line_no);
    let pad = " ".repeat(gutter.len() + col_no.saturating_sub(1));
    let underline_len = span_len.max(1).min(
        target_line
            .len()
            .saturating_sub(col_no.saturating_sub(1))
            .max(1),
    );
    let underline = "^".repeat(underline_len);
    format!("{}{}\n{}{}", gutter, target_line, pad, underline)
}

pub(crate) fn print_report_text(report: &FileLintReport, source: &str, after_fix: bool) {
    use std::io::Write;
    let mut stderr = std::io::stderr().lock();
    if report.diagnostics.is_empty() {
        if report.fixed {
            let _ = writeln!(stderr, "\x1b[32m✓\x1b[0m {}: fixed", report.file);
        } else {
            let _ = writeln!(stderr, "\x1b[32m✓\x1b[0m {}: clean", report.file);
        }
        return;
    }
    for diag in &report.diagnostics {
        let level = if diag.fixable && after_fix {
            "\x1b[32mfixed\x1b[0m"
        } else if diag.fixable {
            "\x1b[33mwarning\x1b[0m"
        } else {
            "\x1b[31merror\x1b[0m"
        };
        let _ = writeln!(
            stderr,
            "{}[{}]: {}\n  --> {}:{}:{}",
            level, diag.code, diag.message, report.file, diag.line, diag.column
        );
        let frame = render_codeframe(
            source,
            diag.line,
            diag.column,
            diag.span_end.saturating_sub(diag.span_start),
        );
        if !frame.is_empty() {
            let _ = writeln!(stderr, "{}\n", frame);
        }
        if let Some(hint) = &diag.hint {
            let _ = writeln!(stderr, "  = hint: {}\n", hint);
        }
    }
}
