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
    let color = crate::output::color_stderr();
    let mut stderr = std::io::stderr().lock();
    if report.diagnostics.is_empty() {
        if report.fixed {
            let _ = writeln!(stderr, "{} {}: fixed", check(color), report.file);
        } else {
            let _ = writeln!(stderr, "{} {}: clean", check(color), report.file);
        }
        return;
    }
    for diag in &report.diagnostics {
        let level = if diag.fixable && after_fix {
            paint(color, "fixed", "32")
        } else if diag.fixable {
            paint(color, "warning", "33")
        } else {
            paint(color, "error", "31")
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

/// ANSI-colored label when `color`, plain text otherwise.
fn paint(color: bool, label: &str, code: &str) -> String {
    if color {
        format!("\x1b[{code}m{label}\x1b[0m")
    } else {
        label.to_string()
    }
}

fn check(color: bool) -> String {
    if color {
        "\x1b[32m✓\x1b[0m".to_string()
    } else {
        "✓".to_string()
    }
}
