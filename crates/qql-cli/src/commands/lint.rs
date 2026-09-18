//! `qql lint` — offline static analysis with error recovery, plan checking, and safe autofix.
//!
//! Pipeline per lint unit (file, stdin, or inline string):
//! 1. [`Parser::parse_all_recovering`] — every syntax error at once, plus the
//!    spans of the statements that parsed.
//! 2. Idiom rules over the recovered AST (explicit `WITH PAYLOAD true`, which is
//!    the default on both QUERY and SCROLL). Each diagnostic points at the exact
//!    clause span, recursing into CTEs and prefetches.
//! 3. [`compile_statement`](qql_plan::routing::compile_statement) on the
//!    param-bound AST — offline plannability with no backend I/O.
//! 4. Canonical-format check.
//!
//! Autofix excises only token spans produced by the real lexer, scoped to
//! statements that parsed (payload) or to the error segment (duplicate WAIT),
//! so string literals, comments, and unparsed regions are never touched.
//! Canonical formatting runs last via `qql_core::fmt`.

use super::convert::source_is_canonical;
use super::lint_fix::apply_fixes;
use super::lint_report::print_report_text;
use qql_core::ast::{PayloadSelector, PrefetchSource, QueryExpr, QueryStmt, Stmt};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use qql_core::token::TokenKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
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

/// The single `--json` shape for every input mode (files, stdin, inline).
/// `content` carries the fixed text only for stdin/inline `--fix --json`,
/// where there is no file to write back to.
#[derive(Debug, serde::Serialize)]
pub struct LintJsonOutput {
    pub ok: bool,
    pub files: Vec<FileLintReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

fn lint_json(ok: bool, files: Vec<FileLintReport>, content: Option<String>) -> String {
    serde_json::to_string_pretty(&LintJsonOutput { ok, files, content })
        .expect("lint report serializes")
}

/// The ONE lint exit rule, shared by every input mode (files, stdin, inline
/// string) with or without `--fix`: a lint unit is clean iff it reports zero
/// diagnostics. Fixability only controls whether `--fix` rewrites the input;
/// a remaining diagnostic — fixable or not — still fails the run.
fn lint_clean(diags: &[LintDiagnostic]) -> bool {
    diags.is_empty()
}

/// A lexed token reduced to its kind and byte span (lifetime-free).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tok {
    kind: TokenKind,
    start: usize,
    end: usize,
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
            Some("clause appears twice — duplicate WAIT clauses can be auto-resolved with --fix")
        }
        "QQL-UNKNOWN-VECTOR" => Some(
            "unknown vector name — check USING and the collection schema (offline: USING <name> AS MULTI)",
        ),
        "QQL-IDIOM-REDUNDANT-PAYLOAD" => Some(
            "QUERY and SCROLL return payloads by default; omit 'WITH PAYLOAD true' or use 'WITH PAYLOAD false' to strip",
        ),
        "QQL-BIND-MISSING-PARAM" => Some(
            "unbound :name / ? placeholder — supply with --param/--params-file or add a '-- qql-params: {...}' header",
        ),
        "QQL-BIND-HEADER" => Some(
            "the leading '-- qql-params: {...}' (or [...]) header must be single-line JSON in the first 10 lines",
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

/// Lex the source into kind/span pairs. Returns `None` when lexing fails, in
/// which case no token-based diagnostic or fix is attempted.
pub(crate) fn lex_kinds(source: &str) -> Option<Vec<Tok>> {
    let mut lexer = Lexer::new(source);
    let mut toks = Vec::new();
    loop {
        match lexer.next_token() {
            Ok(t) => {
                if t.kind == TokenKind::Eof {
                    break;
                }
                toks.push(Tok {
                    kind: t.kind,
                    start: t.span.start,
                    end: t.span.end,
                });
            }
            Err(_) => return None,
        }
    }
    Some(toks)
}

/// True when `source[a..b]` is only token separators (whitespace). The lexer
/// skips `--` comments without emitting tokens, so a comment between two
/// tokens shows up here as a non-blank gap and blocks the match.
fn gap_is_blank(source: &str, a: usize, b: usize) -> bool {
    let (a, b) = (a.min(b), a.max(b));
    source.get(a..b).is_some_and(|g| {
        g.bytes()
            .all(|c| c == b' ' || c == b'\t' || c == b'\n' || c == b'\r')
    })
}

/// Byte spans of explicit `WITH PAYLOAD true` clauses fully inside `[rs, re)`.
/// Matching on lexer tokens (not text) keeps string literals such as
/// `'WITH PAYLOAD TRUE'` and comments out of reach.
pub(crate) fn payload_true_runs(
    source: &str,
    toks: &[Tok],
    rs: usize,
    re: usize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < toks.len() {
        let (a, b, c) = (toks[i], toks[i + 1], toks[i + 2]);
        if a.kind == TokenKind::With
            && b.kind == TokenKind::Payload
            && c.kind == TokenKind::True
            && a.start >= rs
            && c.end <= re
            && gap_is_blank(source, a.end, b.start)
            && gap_is_blank(source, b.end, c.start)
        {
            out.push((a.start, c.end));
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// Byte spans of `WAIT true|false` clauses fully inside `[rs, re)`.
fn wait_runs(source: &str, toks: &[Tok], rs: usize, re: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < toks.len() {
        let (a, b) = (toks[i], toks[i + 1]);
        if a.kind == TokenKind::Wait
            && (b.kind == TokenKind::True || b.kind == TokenKind::False)
            && a.start >= rs
            && b.end <= re
            && gap_is_blank(source, a.end, b.start)
        {
            out.push((a.start, b.end));
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// The `WAIT` clauses in the semicolon-delimited segment around a byte offset
/// (the duplicate-clause error points just past the offending clause, so the
/// segment still contains both copies).
pub(crate) fn wait_runs_around(source: &str, toks: &[Tok], err_pos: usize) -> Vec<(usize, usize)> {
    let mut seg_start = 0;
    let mut seg_end = source.len();
    for t in toks {
        if t.kind == TokenKind::Semicolon {
            if t.end <= err_pos {
                seg_start = t.end;
            } else if t.start >= err_pos {
                seg_end = t.end;
                break;
            }
        }
    }
    wait_runs(source, toks, seg_start, seg_end)
}

/// How many explicit `WITH PAYLOAD true` clauses the AST claims: QUERY output
/// (`None` already means all-payload on the wire, so only `Some(All)` is
/// redundant), recursing into CTEs and inline prefetches with the statement's
/// own output counted last to match source order; plus SCROLL's selector.
/// GROUP LOOKUP payload is deliberately excluded: omitted there lowers to a
/// bare collection name, a different wire shape than explicit `true`.
pub(crate) fn count_redundant_payload(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Query(q) => count_query_payload(q),
        Stmt::Scroll(s) => usize::from(s.with_payload == Some(PayloadSelector::All)),
        _ => 0,
    }
}

fn count_query_payload(q: &QueryStmt) -> usize {
    let mut n = 0;
    for cte in &q.ctes {
        n += count_query_payload(&cte.query);
    }
    n += count_expr_payload(&q.expression);
    n += usize::from(q.output.payload == Some(PayloadSelector::All));
    n
}

fn count_expr_payload(e: &QueryExpr) -> usize {
    let prefetch = match e {
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => return 0,
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => prefetch,
        QueryExpr::Hybrid { .. } => return 0,
    };
    prefetch
        .iter()
        .map(|p| match &p.source {
            PrefetchSource::Query(q) => count_query_payload(q),
            PrefetchSource::Cte(_) => 0,
        })
        .sum()
}

/// Read bind params from a leading `-- qql-params: {...}` (or `[...]`) header:
/// single-line JSON within the first 10 lines, mirroring the VSCode extension.
fn header_params(source: &str) -> Result<Option<serde_json::Value>, String> {
    for line in source.lines().take(10) {
        let after_dashes = match line.trim_start().strip_prefix("--") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        if !after_dashes
            .get(..10)
            .is_some_and(|h| h.eq_ignore_ascii_case("qql-params"))
        {
            continue;
        }
        let rest = after_dashes[10..].trim_start();
        let Some(json_text) = rest.strip_prefix(':') else {
            continue;
        };
        let json_text = json_text.trim();
        if json_text.is_empty() {
            continue;
        }
        return serde_json::from_str(json_text)
            .map(Some)
            .map_err(|e| format!("invalid -- qql-params header JSON: {e}"));
    }
    Ok(None)
}

fn push_diag(
    diagnostics: &mut Vec<LintDiagnostic>,
    source: &str,
    code: String,
    message: String,
    span_start: usize,
    span_end: usize,
    fixable: bool,
) {
    let (line, column) = byte_offset_to_line_col(source, span_start);
    let hint = hint_for(&code).map(String::from);
    diagnostics.push(LintDiagnostic {
        code,
        message,
        line,
        column,
        span_start,
        span_end,
        hint,
        fixable,
    });
}

fn collect_source_diagnostics(
    source: &str,
    filename: &str,
    cli_params: Option<&serde_json::Value>,
) -> (Vec<LintDiagnostic>, Option<String>) {
    let mut diagnostics = Vec::new();

    // Effective params: explicit CLI flags win, else the file header.
    let header = match header_params(source) {
        Ok(h) => h,
        Err(message) => {
            push_diag(
                &mut diagnostics,
                source,
                "QQL-BIND-HEADER".to_string(),
                message,
                0,
                0,
                false,
            );
            None
        }
    };
    let params = cli_params.or(header.as_ref());

    // 1. Syntax analysis with error recovery. The token stream is lexed first
    // so duplicate-clause errors report precise fixability: only WAIT
    // duplicates are autofixed, other duplicates (LIMIT, ...) are not.
    let recovered = Parser::parse_all_recovering(source);
    let toks = lex_kinds(source);
    for err in &recovered.errors {
        let (span_start, span_end) = match err.span {
            Some(s) => (s.start, s.end),
            None => (0, 0),
        };
        let (line, col) = byte_offset_to_line_col(source, span_start);
        let code = err.code.to_string();
        let hint = hint_for(&code).map(|h| h.to_string());
        let fixable = code == "QQL-PARSE-DUPLICATE-CLAUSE"
            && toks
                .as_deref()
                .is_some_and(|t| wait_runs_around(source, t, span_start).len() > 1);
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

    // Token stream reused for clause spans below.

    // 2. Idiom rules + offline plan check on statements that parsed.
    for (stmt, span) in &recovered.statements {
        // Redundant `WITH PAYLOAD true`: point at the clause, not the statement.
        let expected = count_redundant_payload(stmt);
        if expected > 0 {
            let runs = toks
                .as_deref()
                .map(|t| payload_true_runs(source, t, span.start, span.end))
                .unwrap_or_default();
            for (rs, re) in runs.into_iter().take(expected) {
                push_diag(
                    &mut diagnostics,
                    source,
                    "QQL-IDIOM-REDUNDANT-PAYLOAD".to_string(),
                    "redundant 'WITH PAYLOAD true'; QUERY and SCROLL return payload by default"
                        .to_string(),
                    rs,
                    re,
                    true,
                );
            }
        }

        // Validate plannability offline, binding params first so parameterized
        // scripts check exactly as `exec` would run them.
        let mut bound = stmt.clone();
        if let Some(p) = params
            && let Err(bind_err) = qql_core::params_json::bind_stmt_with_params(&mut bound, p)
        {
            let (span_start, span_end) = match bind_err.span {
                Some(s) => (s.start, s.end),
                None => (span.start, span.end),
            };
            push_diag(
                &mut diagnostics,
                source,
                bind_err.code.to_string(),
                bind_err.message.to_string(),
                span_start,
                span_end,
                false,
            );
            continue;
        }
        if let Err(plan_err) = qql_plan::routing::compile_statement(&bound) {
            let (span_start, span_end) = match plan_err.span {
                Some(s) => (s.start, s.end),
                None => (span.start, span.end),
            };
            push_diag(
                &mut diagnostics,
                source,
                plan_err.code.to_string(),
                plan_err.message.to_string(),
                span_start,
                span_end,
                false,
            );
        }
    }

    // 3. Formatting canonical check.
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

pub fn handle_lint(
    target: Option<&str>,
    check: bool,
    fix: bool,
    cli_params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // `--check` is the explicit check-only mode (the default); it exists so CI
    // reads clearly and conflicts with `--fix` at the clap level.
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
            files_to_lint.sort();
        } else if path.is_file() {
            files_to_lint.push((target_str.to_string(), path.to_path_buf()));
        } else if target_str == "-" {
            // Read from stdin
            return lint_stdin(fix, cli_params, json, quiet);
        } else if target_str.ends_with(".qql")
            || ((target_str.contains('/') || target_str.contains('\\'))
                && !target_str.contains(char::is_whitespace))
        {
            // Looks like a path but does not exist: report it instead of
            // misreading the typo as an inline query.
            return Err(format!("no such file or directory: '{target_str}'").into());
        } else {
            // Inline string check
            return lint_string(target_str, fix, cli_params, json, quiet);
        }
    } else {
        // No target: lint the working tree when interactive (matching the
        // documented `qql lint` default), else consume piped stdin.
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            for entry in walkdir(Path::new("."))? {
                if entry.extension().and_then(|s| s.to_str()) == Some("qql") {
                    files_to_lint.push((entry.display().to_string(), entry));
                }
            }
            files_to_lint.sort();
        } else {
            return lint_stdin(fix, cli_params, json, quiet);
        }
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
            // Re-lint after fixes; the exit rule counts every remaining
            // diagnostic (see `lint_clean`), not just the unfixable ones.
            let (diags, _) = collect_source_diagnostics(&fixed_content, display_name, cli_params);
            total_errors += diags.len();
            let valid = lint_clean(&diags);
            if !json {
                print_report_text(
                    &FileLintReport {
                        file: display_name.clone(),
                        valid,
                        fixed: fix_count > 0,
                        diagnostics: diags.clone(),
                    },
                    &fixed_content,
                    true,
                );
            }
            reports.push(FileLintReport {
                file: display_name.clone(),
                valid,
                fixed: fix_count > 0,
                diagnostics: diags,
            });
        } else {
            let (diags, _) = collect_source_diagnostics(&content, display_name, cli_params);
            let err_count = diags.len();
            total_errors += err_count;
            let valid = lint_clean(&diags);
            if !json {
                print_report_text(
                    &FileLintReport {
                        file: display_name.clone(),
                        valid,
                        fixed: false,
                        diagnostics: diags.clone(),
                    },
                    &content,
                    false,
                );
            }
            reports.push(FileLintReport {
                file: display_name.clone(),
                valid,
                fixed: false,
                diagnostics: diags,
            });
        }
    }

    if json {
        let ok = total_errors == 0;
        println!("{}", lint_json(ok, reports, None));
        if !ok {
            return Err("lint errors found".into());
        }
        return Ok(());
    }

    if fix && total_fixed > 0 && !quiet {
        eprintln!(
            "Applied {} safe fix(es) across {} file(s).",
            total_fixed,
            files_to_lint.len()
        );
    }

    if total_errors > 0 {
        return Err(format!("lint completed with {} error(s)", total_errors).into());
    }

    if !quiet {
        if total_fixed > 0 {
            eprintln!("Checked {} file(s): all fixed.", files_to_lint.len());
        } else {
            eprintln!("Checked {} file(s): all clean.", files_to_lint.len());
        }
    }

    Ok(())
}

fn lint_stdin(
    fix: bool,
    cli_params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;

    if fix {
        let (fixed_content, _) = apply_fixes(&buf);
        // Re-lint what was actually produced: any remaining diagnostic fails
        // (see `lint_clean`), not just the unfixable ones.
        let (diags, _) = collect_source_diagnostics(&fixed_content, "<stdin>", cli_params);
        let report = FileLintReport {
            file: "<stdin>".to_string(),
            valid: lint_clean(&diags),
            fixed: fixed_content != buf,
            diagnostics: diags,
        };
        let valid = report.valid;
        if json {
            println!(
                "{}",
                lint_json(valid, vec![report], Some(fixed_content.clone()),)
            );
        } else {
            print!("{}", fixed_content);
        }
        if !valid {
            return Err("lint found error(s)".into());
        }
        return Ok(());
    }

    let (diags, _) = collect_source_diagnostics(&buf, "<stdin>", cli_params);
    let valid = lint_clean(&diags);
    if json {
        println!(
            "{}",
            lint_json(
                valid,
                vec![FileLintReport {
                    file: "<stdin>".to_string(),
                    valid,
                    fixed: false,
                    diagnostics: diags,
                }],
                None,
            )
        );
    } else {
        print_report_text(
            &FileLintReport {
                file: "<stdin>".to_string(),
                valid,
                fixed: false,
                diagnostics: diags.clone(),
            },
            &buf,
            false,
        );
    }

    if !valid {
        return Err("lint found error(s)".into());
    }
    if !quiet && !json {
        eprintln!("\x1b[32m✓\x1b[0m <stdin>: clean");
    }
    Ok(())
}

fn lint_string(
    query: &str,
    fix: bool,
    cli_params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if fix {
        let (fixed_content, _) = apply_fixes(query);
        let (diags, _) = collect_source_diagnostics(&fixed_content, "<query>", cli_params);
        let report = FileLintReport {
            file: "<query>".to_string(),
            valid: lint_clean(&diags),
            fixed: fixed_content != query,
            diagnostics: diags,
        };
        let valid = report.valid;
        if json {
            println!(
                "{}",
                lint_json(valid, vec![report], Some(fixed_content.clone()),)
            );
        } else {
            print!("{}", fixed_content);
        }
        if !valid {
            return Err("lint found error(s)".into());
        }
        return Ok(());
    }

    let (diags, _) = collect_source_diagnostics(query, "<query>", cli_params);
    let valid = lint_clean(&diags);
    if json {
        println!(
            "{}",
            lint_json(
                valid,
                vec![FileLintReport {
                    file: "<query>".to_string(),
                    valid,
                    fixed: false,
                    diagnostics: diags,
                }],
                None,
            )
        );
    } else {
        print_report_text(
            &FileLintReport {
                file: "<query>".to_string(),
                valid,
                fixed: false,
                diagnostics: diags.clone(),
            },
            query,
            false,
        );
    }
    if !valid {
        return Err("lint found error(s)".into());
    }
    if !quiet && !json {
        eprintln!("\x1b[32m✓\x1b[0m query: clean");
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
    files.sort();
    Ok(files)
}
