//! `qql lint` — offline static analysis, syntax recovery, plan checking, and autofix.

use super::convert::source_is_canonical;
use qql_core::ast::{PayloadSelector, Stmt};
use qql_core::parser::Parser;
use std::path::{Path, PathBuf};

#[derive(Debug, serde::Serialize)]
pub struct LintDiagnostic {
    pub code: String,
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub span_start: usize,
    pub span_end: usize,
    pub hint: Option<String>,
    pub fixable: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct FileLintReport {
    pub file: String,
    pub valid: bool,
    pub fixed: bool,
    pub diagnostics: Vec<LintDiagnostic>,
}

fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn hint_for(code: &str) -> Option<&'static str> {
    match code {
        "QQL-PARSE-VECTOR-DIFF" => Some(
            "per-vector diffs are WITH VECTOR [name] (HNSW (...) | QUANTIZATION (...) | VECTOR (...))",
        ),
        "QQL-PLAN-VECTOR-DIFF" => Some(
            "the wire cannot express this diff (e.g. datatype) — set it at CREATE COLLECTION time",
        ),
        "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF" => Some(
            "edge only applies per-vector hnsw_config — run this statement against server Qdrant",
        ),
        "QQL-EDGE-UNSUPPORTED-SPARSE-DIFF" => {
            Some("edge has no per-sparse setter — run this statement against server Qdrant")
        }
        "QQL-PARSE-DUPLICATE-CLAUSE" => {
            Some("clause appears twice — keep a single trailing clause")
        }
        "QQL-UNKNOWN-VECTOR" => Some(
            "unknown vector name — check USING and the collection schema (offline: USING <name> AS MULTI)",
        ),
        "QQL-IDIOM-REDUNDANT-PAYLOAD" => Some(
            "QUERY returns payloads by default; omit 'WITH PAYLOAD true' or use 'WITH PAYLOAD false' to strip",
        ),
        "QQL-BIND-MISSING-PARAM" => Some(
            "unbound :name / ? placeholder — supply with --param or add a '-- qql-params: {...}' header",
        ),
        "QQL-FMT-NON-CANONICAL" => {
            Some("run `qql lint --fix` or `qql fmt --write` to canonicalize")
        }
        _ if code.starts_with("QQL-BIND-") => {
            Some("unbound placeholder — supply parameter values or scaffold header")
        }
        _ => None,
    }
}

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

/// Strip redundant `WITH PAYLOAD true` (case-insensitive) while preserving trailing comments/semicolons.
fn strip_redundant_payload(source: &str) -> (String, bool) {
    let re = regex_lite_or_manual_strip(source);
    let modified = re != source;
    (re, modified)
}

fn regex_lite_or_manual_strip(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut remaining = source;
    while let Some(idx) = remaining.to_uppercase().find("WITH PAYLOAD TRUE") {
        let before = &remaining[..idx];
        let after = &remaining[idx + "WITH PAYLOAD TRUE".len()..];
        // Ensure word boundaries
        let is_word_before = idx > 0 && remaining.as_bytes()[idx - 1].is_ascii_alphanumeric();
        let is_word_after = !after.is_empty() && after.as_bytes()[0].is_ascii_alphanumeric();
        if is_word_before || is_word_after {
            out.push_str(&remaining[..idx + "WITH PAYLOAD TRUE".len()]);
            remaining = after;
            continue;
        }
        // Trim one preceding whitespace run if present
        let trimmed_before = before.trim_end_matches([' ', '\t']);
        out.push_str(trimmed_before);
        remaining = after;
    }
    out.push_str(remaining);
    out
}

/// Remove duplicate WAIT clauses on a single line.
fn remove_duplicate_wait(source: &str) -> (String, bool) {
    let mut changed = false;
    let mut out_lines = Vec::new();
    for line in source.lines() {
        let upper = line.to_uppercase();
        let mut matches = Vec::new();
        let mut search_from = 0;
        while let Some(pos) = upper[search_from..].find("WAIT") {
            let actual_pos = search_from + pos;
            let after = &upper[actual_pos + 4..];
            let after_trimmed = after.trim_start();
            if after_trimmed.starts_with("TRUE") || after_trimmed.starts_with("FALSE") {
                matches.push(actual_pos);
            }
            search_from = actual_pos + 4;
        }
        if matches.len() > 1 {
            changed = true;
            // Delete the duplicate (second/last) WAIT clause
            let last_idx = matches[matches.len() - 1];
            let before = line[..last_idx].trim_end();
            let after = &line[last_idx..];
            let cut_end = if let Some(semi) = after.find(';') {
                last_idx + semi
            } else {
                line.len()
            };
            let mut new_line = String::new();
            new_line.push_str(before);
            new_line.push_str(&line[cut_end..]);
            out_lines.push(new_line);
        } else {
            out_lines.push(line.to_string());
        }
    }
    let mut res = out_lines.join("\n");
    if source.ends_with('\n') {
        res.push('\n');
    }
    (res, changed)
}

fn collect_source_diagnostics(
    source: &str,
    filename: &str,
) -> (Vec<LintDiagnostic>, Option<String>) {
    let mut diagnostics = Vec::new();

    // 1. Syntax analysis with error recovery
    let recovered = Parser::parse_all_recovering(source);
    for err in &recovered.errors {
        let (span_start, span_end) = match err.span {
            Some(s) => (s.start, s.end),
            None => (0, 0),
        };
        let (line, col) = byte_offset_to_line_col(source, span_start);
        let code = err.code.to_string();
        let hint = hint_for(&code).map(|h| h.to_string());
        let fixable = code == "QQL-PARSE-DUPLICATE-CLAUSE";
        diagnostics.push(LintDiagnostic {
            code,
            message: err.message.to_string(),
            line,
            column: col,
            span_start,
            span_end,
            hint,
            fixable,
        });
    }

    // 2. Planning and semantic check on parsed statements
    for (stmt, span) in &recovered.statements {
        if let Stmt::Query(q) = stmt
            && q.output.payload == Some(PayloadSelector::All)
        {
            let (line, col) = byte_offset_to_line_col(source, span.start);
            diagnostics.push(LintDiagnostic {
                code: "QQL-IDIOM-REDUNDANT-PAYLOAD".to_string(),
                message: "redundant 'WITH PAYLOAD true'; QUERY returns payload by default"
                    .to_string(),
                line,
                column: col,
                span_start: span.start,
                span_end: span.end,
                hint: hint_for("QQL-IDIOM-REDUNDANT-PAYLOAD").map(String::from),
                fixable: true,
            });
        }

        // Validate plannability offline
        if let Err(plan_err) = qql_plan::routing::compile_statement(stmt) {
            let (span_start, span_end) = (span.start, span.end);
            let (line, col) = byte_offset_to_line_col(source, span_start);
            let code = plan_err.code.to_string();
            let hint = hint_for(&code).map(String::from);
            diagnostics.push(LintDiagnostic {
                code,
                message: plan_err.message.to_string(),
                line,
                column: col,
                span_start,
                span_end,
                hint,
                fixable: false,
            });
        }
    }

    // 3. Formatting canonical check
    let formatted_candidate = match qql_core::fmt::format(source) {
        Ok(formatted) => {
            if !source_is_canonical(source, &formatted) {
                let (line, col) = (1, 1);
                diagnostics.push(LintDiagnostic {
                    code: "QQL-FMT-NON-CANONICAL".to_string(),
                    message: format!("file '{}' is not formatted canonically", filename),
                    line,
                    column: col,
                    span_start: 0,
                    span_end: 0,
                    hint: hint_for("QQL-FMT-NON-CANONICAL").map(String::from),
                    fixable: true,
                });
            }
            Some(formatted)
        }
        Err(_) => None,
    };

    (diagnostics, formatted_candidate)
}

fn apply_fixes(source: &str) -> (String, usize) {
    let mut fixes = 0;
    let (after_wait, wait_changed) = remove_duplicate_wait(source);
    if wait_changed {
        fixes += 1;
    }
    let (after_payload, payload_changed) = strip_redundant_payload(&after_wait);
    if payload_changed {
        fixes += 1;
    }
    if let Ok(formatted) = qql_core::fmt::format(&after_payload) {
        if !source_is_canonical(&after_payload, &formatted) {
            fixes += 1;
        }
        (formatted, fixes)
    } else {
        (after_payload, fixes)
    }
}

pub fn handle_lint(
    target: Option<&str>,
    check: bool,
    fix: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = check;
    let mut files_to_lint: Vec<(String, PathBuf)> = Vec::new();

    if let Some(target_str) = target {
        let path = Path::new(target_str);
        if path.is_dir() {
            for entry in walkdir(path)? {
                if entry.extension().and_then(|s| s.to_str()) == Some("qql") {
                    files_to_lint.push((entry.display().to_string(), entry));
                }
            }
        } else if path.is_file() {
            files_to_lint.push((target_str.to_string(), path.to_path_buf()));
        } else if target_str == "-" {
            // Read from stdin
            return lint_stdin(fix, json, quiet);
        } else {
            // Inline string check
            return lint_string(target_str, json, quiet);
        }
    } else {
        // Stdin
        return lint_stdin(fix, json, quiet);
    }

    if files_to_lint.is_empty() {
        if !quiet {
            eprintln!("No .qql files found to lint.");
        }
        return Ok(());
    }

    let mut reports = Vec::new();
    let mut total_errors = 0;
    let mut total_fixed = 0;

    for (display_name, path) in &files_to_lint {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read '{}': {}", display_name, e))?;

        if fix {
            let (fixed_content, fix_count) = apply_fixes(&content);
            if fix_count > 0 && fixed_content != content {
                std::fs::write(path, &fixed_content)
                    .map_err(|e| format!("failed to write '{}': {}", display_name, e))?;
                total_fixed += fix_count;
            }
            // Re-lint after fixes
            let (diags, _) = collect_source_diagnostics(&fixed_content, display_name);
            let unfixable_count = diags.iter().filter(|d| !d.fixable).count();
            total_errors += unfixable_count;
            reports.push(FileLintReport {
                file: display_name.clone(),
                valid: unfixable_count == 0,
                fixed: fix_count > 0,
                diagnostics: diags,
            });
        } else {
            let (diags, _) = collect_source_diagnostics(&content, display_name);
            let err_count = diags.len();
            total_errors += err_count;
            reports.push(FileLintReport {
                file: display_name.clone(),
                valid: err_count == 0,
                fixed: false,
                diagnostics: diags,
            });
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
        if total_errors > 0 {
            return Err("lint errors found".into());
        }
        return Ok(());
    }

    // Terminal render
    for report in &reports {
        if report.diagnostics.is_empty() {
            if !quiet && !fix {
                println!("\x1b[32m✓\x1b[0m {}: clean", report.file);
            } else if !quiet && report.fixed {
                println!("\x1b[32m✓\x1b[0m {}: fixed", report.file);
            }
            continue;
        }

        let content = std::fs::read_to_string(&report.file).unwrap_or_default();
        for diag in &report.diagnostics {
            let level = if diag.fixable && fix {
                "\x1b[32mfixed\x1b[0m"
            } else if diag.fixable {
                "\x1b[33mwarning\x1b[0m"
            } else {
                "\x1b[31merror\x1b[0m"
            };
            println!(
                "{}[{}]: {}\n  --> {}:{}:{}",
                level, diag.code, diag.message, report.file, diag.line, diag.column
            );
            let frame = render_codeframe(
                &content,
                diag.line,
                diag.column,
                diag.span_end.saturating_sub(diag.span_start),
            );
            if !frame.is_empty() {
                println!("{}\n", frame);
            }
            if let Some(hint) = &diag.hint {
                println!("  = hint: {}\n", hint);
            }
        }
    }

    if fix && total_fixed > 0 && !quiet {
        println!(
            "Applied {} safe fix(es) across {} file(s).",
            total_fixed,
            files_to_lint.len()
        );
    }

    if total_errors > 0 {
        return Err(format!("lint completed with {} error(s)", total_errors).into());
    }

    if !quiet && !fix {
        println!("Checked {} file(s): all clean.", files_to_lint.len());
    }

    Ok(())
}

fn lint_stdin(fix: bool, json: bool, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let (diags, _) = collect_source_diagnostics(&buf, "<stdin>");

    if fix {
        let (fixed_content, _) = apply_fixes(&buf);
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "file": "<stdin>",
                    "fixed": fixed_content != buf,
                    "content": fixed_content,
                    "diagnostics": diags,
                })
            );
        } else {
            print!("{}", fixed_content);
        }
        return Ok(());
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "file": "<stdin>",
                "valid": diags.is_empty(),
                "diagnostics": diags,
            })
        );
    } else {
        for diag in &diags {
            eprintln!(
                "\x1b[31merror\x1b[0m[{}]: {}\n  --> <stdin>:{}:{}",
                diag.code, diag.message, diag.line, diag.column
            );
            if let Some(hint) = &diag.hint {
                eprintln!("  = hint: {}", hint);
            }
        }
    }

    if !diags.is_empty() {
        return Err(format!("lint found {} error(s)", diags.len()).into());
    }
    if !quiet && !json {
        eprintln!("\x1b[32m✓\x1b[0m <stdin>: clean");
    }
    Ok(())
}

fn lint_string(query: &str, json: bool, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (diags, _) = collect_source_diagnostics(query, "<query>");
    if json {
        println!(
            "{}",
            serde_json::json!({
                "valid": diags.is_empty(),
                "diagnostics": diags,
            })
        );
    } else {
        for diag in &diags {
            eprintln!(
                "\x1b[31merror\x1b[0m[{}]: {}\n  --> <query>:{}:{}",
                diag.code, diag.message, diag.line, diag.column
            );
            if let Some(hint) = &diag.hint {
                eprintln!("  = hint: {}", hint);
            }
        }
    }
    if !diags.is_empty() {
        return Err(format!("lint found {} error(s)", diags.len()).into());
    }
    if !quiet && !json {
        println!("\x1b[32m✓\x1b[0m query: clean");
    }
    Ok(())
}

fn walkdir(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walkdir(&path)?);
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}
