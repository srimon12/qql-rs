use qql_core::error::QqlError;
use qql_core::parser::Parser;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const CASE_MARKER: &str = "-- @case ";
const ERROR_MARKER: &str = "-- @error ";
const AST_SCHEMA: &str = "qql.ast/v1";
const FLOAT_DECIMAL_PLACES: i32 = 6;

fn main() {
    if let Err(error) = run() {
        eprintln!("qql-conformance: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "check".to_owned());
    let spec_dir = args
        .next()
        .map(PathBuf::from)
        .or_else(|| env::var_os("QQL_LANGUAGE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("language/v1"));

    if args.next().is_some() {
        return Err(usage().into());
    }

    match command.as_str() {
        "check" => check(&spec_dir, true),
        "generate" => generate(&spec_dir),
        "help" | "-h" | "--help" => {
            println!("{}", usage());
            Ok(())
        }
        _ => Err(format!("unknown command '{command}'\n\n{}", usage()).into()),
    }
}

fn usage() -> &'static str {
    "Usage: qql-conformance <check|generate> [LANGUAGE_DIR]\n\
     \n\
     check     Parse every valid case, reject every invalid case, and compare AST + canonical-format snapshots.\n\
     generate  Validate all cases, then regenerate fixtures/expected/*.json and fixtures/formatted/*.txt."
}

fn check(spec_dir: &Path, require_expected: bool) -> Result<(), Box<dyn Error>> {
    let mut report = inspect(spec_dir, require_expected)?;
    if require_expected {
        // Stale-snapshot safety: an expected snapshot without a matching
        // fixture means the fixture was deleted/renamed; `check` must fail
        // until the stale file is removed (or regenerated).
        let valid_dir = spec_dir.join("fixtures/valid");
        let expected_dir = spec_dir.join("fixtures/expected");
        let formatted_dir = spec_dir.join("fixtures/formatted");
        for stale in stale_snapshots(&valid_dir, &expected_dir)? {
            report.failures.push(format!(
                "{}: stale AST snapshot (no matching fixture); remove it or run `cargo run -p qql-conformance -- generate`",
                stale.display()
            ));
        }
        for stale in stale_formatted(&valid_dir, &formatted_dir)? {
            report.failures.push(format!(
                "{}: stale canonical-format snapshot (no matching fixture); remove it or run `cargo run -p qql-conformance -- generate`",
                stale.display()
            ));
        }
    }
    finish_check(&report)
}

fn finish_check(report: &Report) -> Result<(), Box<dyn Error>> {
    if report.failures.is_empty() {
        println!(
            "conformant: {} valid files ({} statements), {} invalid cases, {} AST snapshots, {} canonical formats",
            report.valid_files,
            report.valid_statements,
            report.invalid_cases,
            report.expected_files,
            report.formatted_files
        );
        return Ok(());
    }

    for failure in &report.failures {
        eprintln!("{failure}");
    }
    Err(format!("{} conformance failure(s)", report.failures.len()).into())
}

fn generate(spec_dir: &Path) -> Result<(), Box<dyn Error>> {
    let report = inspect(spec_dir, false)?;
    if !report.failures.is_empty() {
        for failure in &report.failures {
            eprintln!("{failure}");
        }
        return Err(format!(
            "refusing to generate snapshots with {} fixture failure(s)",
            report.failures.len()
        )
        .into());
    }

    let valid_dir = spec_dir.join("fixtures/valid");
    let expected_dir = spec_dir.join("fixtures/expected");
    let formatted_dir = spec_dir.join("fixtures/formatted");
    fs::create_dir_all(&expected_dir)?;
    fs::create_dir_all(&formatted_dir)?;

    let mut written = 0;
    for fixture in qql_files(&valid_dir)? {
        let source = fs::read_to_string(&fixture)?;
        let statements =
            parse_and_validate(&source).map_err(|error| format_parse_failure(&fixture, &error))?;
        let json = serde_json::to_string_pretty(&canonical_ast(&statements)?)?;
        let output = expected_path(&fixture, &valid_dir, &expected_dir);
        write_atomic(&output, &(json + "\n"))?;
        // Canonical-format golden: exactly what `fmt::format` produces for the
        // fixture. Shared contract for every formatter implementation (native
        // and WASM) — a stale bundle diverges from this text.
        let formatted = qql_core::fmt::format(&source)
            .map_err(|error| format_parse_failure(&fixture, &error))?;
        let formatted_output = formatted_path(&fixture, &valid_dir, &formatted_dir);
        write_atomic(&formatted_output, &formatted)?;
        written += 1;
    }

    // Stale-snapshot safety: remove snapshots whose fixture has been deleted
    // or renamed, so a dead snapshot cannot keep passing `check`.
    for stale in stale_snapshots(&valid_dir, &expected_dir)? {
        fs::remove_file(&stale)?;
        println!("removed stale snapshot {}", stale.display());
    }
    for stale in stale_formatted(&valid_dir, &formatted_dir)? {
        fs::remove_file(&stale)?;
        println!(
            "removed stale canonical-format snapshot {}",
            stale.display()
        );
    }

    println!(
        "generated {written} AST snapshot(s) in {} and {written} canonical format(s) in {}",
        expected_dir.display(),
        formatted_dir.display()
    );
    Ok(())
}

#[derive(Default)]
struct Report {
    valid_files: usize,
    valid_statements: usize,
    invalid_cases: usize,
    expected_files: usize,
    formatted_files: usize,
    failures: Vec<String>,
}

fn inspect(spec_dir: &Path, require_expected: bool) -> Result<Report, Box<dyn Error>> {
    let valid_dir = spec_dir.join("fixtures/valid");
    let invalid_dir = spec_dir.join("fixtures/invalid");
    let expected_dir = spec_dir.join("fixtures/expected");
    let formatted_dir = spec_dir.join("fixtures/formatted");
    ensure_directory(&valid_dir)?;
    ensure_directory(&invalid_dir)?;

    let mut report = Report::default();

    for fixture in qql_files(&valid_dir)? {
        report.valid_files += 1;
        let source = fs::read_to_string(&fixture)?;
        match parse_and_validate(&source) {
            Ok(statements) => {
                report.valid_statements += statements.len();
                if require_expected {
                    compare_expected(
                        &fixture,
                        &valid_dir,
                        &expected_dir,
                        &statements,
                        &mut report,
                    )?;
                    compare_formatted(&fixture, &valid_dir, &formatted_dir, &source, &mut report)?;
                }
            }
            Err(error) => report.failures.push(format_parse_failure(&fixture, &error)),
        }
    }

    for fixture in qql_files(&invalid_dir)? {
        let source = fs::read_to_string(&fixture)?;
        match invalid_cases(&source) {
            Ok(cases) => {
                for case in cases {
                    report.invalid_cases += 1;
                    match parse_and_validate(&case.source) {
                        Ok(_) => report.failures.push(format!(
                            "{} [{}]: expected rejection, but parsing succeeded",
                            fixture.display(),
                            case.name
                        )),
                        Err(error) => {
                            if let Some(expected) = case.expected_error.as_deref()
                                && error.code != expected
                            {
                                report.failures.push(format!(
                                    "{} [{}]: expected error {}, got {} ({})",
                                    fixture.display(),
                                    case.name,
                                    expected,
                                    error.code,
                                    error.message
                                ));
                            }
                        }
                    }
                }
            }
            Err(error) => report
                .failures
                .push(format!("{}: {error}", fixture.display())),
        }
    }

    Ok(report)
}

fn parse_and_validate(source: &str) -> Result<Vec<qql_core::ast::Stmt>, QqlError> {
    let statements = Parser::parse_all(source)?;
    for statement in &statements {
        qql_plan::plan::plan(statement)?;
    }
    Ok(statements)
}

fn compare_expected(
    fixture: &Path,
    valid_dir: &Path,
    expected_dir: &Path,
    statements: &[qql_core::ast::Stmt],
    report: &mut Report,
) -> Result<(), Box<dyn Error>> {
    let expected = expected_path(fixture, valid_dir, expected_dir);
    if !expected.is_file() {
        report
            .failures
            .push(format!("{}: missing AST snapshot", expected.display()));
        return Ok(());
    }
    report.expected_files += 1;

    let expected_json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&expected)?)?;
    let actual_json = canonical_ast(statements)?;
    if actual_json != expected_json {
        report.failures.push(format!(
            "{}: AST snapshot differs; run `cargo run -p qql-conformance -- generate {}`",
            expected.display(),
            expected_dir
                .parent()
                .and_then(Path::parent)
                .unwrap_or(expected_dir)
                .display()
        ));
    }
    Ok(())
}

fn canonical_ast(statements: &[qql_core::ast::Stmt]) -> Result<serde_json::Value, QqlError> {
    let mut statements = serde_json::to_value(statements).map_err(|error| {
        QqlError::validation(
            "QQL-CONFORMANCE-AST",
            format!("failed to normalize AST: {error}"),
            None,
        )
    })?;
    normalize_numbers(&mut statements);
    Ok(serde_json::json!({
        "schema": AST_SCHEMA,
        "statements": statements,
    }))
}

/// Canonical-format golden path for a fixture, mirroring its relative
/// location under `fixtures/valid`.
fn formatted_path(fixture: &Path, valid_dir: &Path, formatted_dir: &Path) -> PathBuf {
    let relative = fixture.strip_prefix(valid_dir).unwrap_or(fixture);
    formatted_dir.join(relative).with_extension("txt")
}

/// Verify a fixture's canonical-format golden against `fmt::format`.
fn compare_formatted(
    fixture: &Path,
    valid_dir: &Path,
    formatted_dir: &Path,
    source: &str,
    report: &mut Report,
) -> Result<(), Box<dyn Error>> {
    let golden = formatted_path(fixture, valid_dir, formatted_dir);
    if !golden.is_file() {
        report.failures.push(format!(
            "{}: missing canonical-format snapshot",
            golden.display()
        ));
        return Ok(());
    }
    report.formatted_files += 1;

    let expected = fs::read_to_string(&golden)?;
    let actual =
        qql_core::fmt::format(source).map_err(|error| format_parse_failure(fixture, &error))?;
    if actual != expected {
        report.failures.push(format!(
            "{}: canonical format differs from {}; run `cargo run -p qql-conformance -- generate`",
            fixture.display(),
            golden.display()
        ));
    }
    Ok(())
}

/// Canonical-format goldens whose `*.qql` fixture no longer exists.
fn stale_formatted(valid_dir: &Path, formatted_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stale = Vec::new();
    if !formatted_dir.is_dir() {
        return Ok(stale);
    }
    for entry in text_files(formatted_dir)? {
        let relative = entry
            .strip_prefix(formatted_dir)
            .unwrap_or(&entry)
            .with_extension("qql");
        let fixture = valid_dir.join(relative);
        if !fixture.is_file() {
            stale.push(entry);
        }
    }
    Ok(stale)
}

fn text_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files_with_extension(directory, "txt", &mut files)?;
    files.sort();
    Ok(files)
}

fn normalize_numbers(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) if number.is_f64() => {
            let Some(numeric) = number.as_f64() else {
                return;
            };
            let factor = 10_f64.powi(FLOAT_DECIMAL_PLACES);
            let rounded = (numeric * factor).round() / factor;
            if let Some(number) = serde_json::Number::from_f64(rounded) {
                *value = serde_json::Value::Number(number);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_numbers(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                normalize_numbers(value);
            }
        }
        _ => {}
    }
}

#[derive(Debug, PartialEq, Eq)]
struct InvalidCase {
    name: String,
    expected_error: Option<String>,
    source: String,
}

fn invalid_cases(source: &str) -> Result<Vec<InvalidCase>, String> {
    let mut cases = Vec::new();
    let mut current: Option<InvalidCase> = None;

    for line in source.lines() {
        if let Some(name) = line.trim().strip_prefix(CASE_MARKER) {
            finish_case(&mut cases, current.take())?;
            if name.trim().is_empty() {
                return Err("invalid fixture has an empty @case name".to_owned());
            }
            current = Some(InvalidCase {
                name: name.trim().to_owned(),
                expected_error: None,
                source: String::new(),
            });
            continue;
        }

        if let Some(code) = line.trim().strip_prefix(ERROR_MARKER) {
            let case = current
                .as_mut()
                .ok_or_else(|| "@error must follow an @case marker".to_owned())?;
            if case.expected_error.is_some() {
                return Err(format!("case '{}' has multiple @error markers", case.name));
            }
            case.expected_error = Some(code.trim().to_owned());
            continue;
        }

        if let Some(case) = current.as_mut() {
            case.source.push_str(line);
            case.source.push('\n');
        } else if !line.trim().is_empty() && !line.trim().starts_with("--") {
            return Err("invalid fixtures must use `-- @case <name>` markers".to_owned());
        }
    }
    finish_case(&mut cases, current)?;

    if cases.is_empty() {
        return Err("invalid fixture contains no @case markers".to_owned());
    }
    Ok(cases)
}

fn finish_case(cases: &mut Vec<InvalidCase>, case: Option<InvalidCase>) -> Result<(), String> {
    let Some(mut case) = case else {
        return Ok(());
    };
    case.source = case.source.trim().to_owned();
    if case.source.is_empty() {
        return Err(format!("case '{}' has no QQL source", case.name));
    }
    cases.push(case);
    Ok(())
}

/// Recursively collect `*.qql` files under `directory` (sorted).
fn qql_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_qql_files(directory, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_qql_files(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_qql_files(&path, out)?;
        } else if path.extension() == Some(OsStr::new("qql")) {
            out.push(path);
        }
    }
    Ok(())
}

/// Expected snapshot path for a fixture, preserving the fixture's relative
/// location under `fixtures/valid` (so nested fixture directories work).
fn expected_path(fixture: &Path, valid_dir: &Path, expected_dir: &Path) -> PathBuf {
    let relative = fixture.strip_prefix(valid_dir).unwrap_or(fixture);
    expected_dir.join(relative).with_extension("json")
}

/// Expected snapshots whose `*.qql` fixture no longer exists.
fn stale_snapshots(valid_dir: &Path, expected_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stale = Vec::new();
    for entry in qql_json_files(expected_dir)? {
        let relative = entry
            .strip_prefix(expected_dir)
            .unwrap_or(&entry)
            .with_extension("qql");
        let fixture = valid_dir.join(relative);
        if !fixture.is_file() {
            stale.push(entry);
        }
    }
    Ok(stale)
}

fn qql_json_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect_files_with_extension(directory, "json", &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_with_extension(
    directory: &Path,
    extension: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_extension(&path, extension, out)?;
        } else if path.extension() == Some(OsStr::new(extension)) {
            out.push(path);
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("missing directory {}", path.display()).into())
    }
}

fn format_parse_failure(path: &Path, error: &QqlError) -> String {
    format!(
        "{}: {} {:?}: {}{}",
        path.display(),
        error.code,
        error.kind,
        error.message,
        error
            .span
            .map(|span| format!(" at {}..{}", span.start, span.end))
            .unwrap_or_default()
    )
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod grammar_gate {
    // Executable grammar coverage (F-4): compile the canonical
    // language/v1/grammar.pest with pest (test-only dev-dependencies; pest is
    // never a runtime dependency of qql-core) and assert the fixture corpus
    // against it, so grammar↔runtime drift becomes a test failure.

    use pest::Parser;
    use pest_derive::Parser as PestParser;
    use qql_core::parser::Parser as QqlParser;
    use std::path::{Path, PathBuf};

    #[derive(PestParser)]
    #[grammar = "../../language/v1/grammar.pest"]
    struct QqlGrammar;

    fn spec_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/v1")
    }

    fn qql_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        collect(dir, &mut out);
        out.sort();
        out
    }

    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, out);
            } else if path.extension().is_some_and(|e| e == "qql") {
                out.push(path);
            }
        }
    }

    #[test]
    fn every_valid_fixture_parses_as_script() {
        let valid = spec_dir().join("fixtures/valid");
        let mut failures = Vec::new();
        for fixture in qql_files(&valid) {
            let source = std::fs::read_to_string(&fixture).unwrap();
            if QqlGrammar::parse(Rule::script, &source).is_err() {
                failures.push(fixture.display().to_string());
            }
        }
        assert!(
            failures.is_empty(),
            "{} valid fixture(s) are rejected by grammar.pest:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn grammar_rejected_cases_are_rejected_by_the_runtime() {
        // The grammar is the canonical contract: anything it rejects must also
        // be rejected by the reference parser. Cases the grammar accepts are
        // validation-level rejections pinned by their @error codes.
        let spec = spec_dir();
        let mut checked = 0;
        for fixture in qql_files(&spec.join("fixtures/invalid")) {
            let source = std::fs::read_to_string(&fixture).unwrap();
            let cases = super::invalid_cases(&source).unwrap();
            for case in cases {
                if QqlGrammar::parse(Rule::single_statement, &case.source).is_ok() {
                    continue; // grammar-accepted: validation/plan-level rejection
                }
                checked += 1;
                assert!(
                    QqlParser::parse_all(&case.source).is_err(),
                    "grammar rejects case '{}' ({}) but the runtime accepts it",
                    case.name,
                    fixture.display()
                );
            }
        }
        assert!(
            checked > 0,
            "expected at least one grammar-rejected invalid case"
        );
    }

    #[test]
    fn grammar_and_runtime_agree_on_aligned_divergences() {
        // Cases from the grammar-alignment audit (F-2, F-5) fixed in this pass:
        // grammar and runtime must now agree on every input.
        let both_reject = [
            // F-5a count clause order (grammar order: WHERE → SHARD → WITH).
            "COUNT FROM docs WITH (exact = true) SHARD 'x';",
            "COUNT FROM docs SHARD 'a' SHARD 'b';",
            // F-5b closed field_type enum.
            "CREATE INDEX ON COLLECTION docs FOR title TYPE banana;",
            // F-5c feedback params must be exactly a, b, c in order.
            "QUERY RELEVANCE FEEDBACK TARGET TEXT 'x' FEEDBACK ((TEXT 'y', 1.0)) STRATEGY NAIVE (a = 1, c = 2, b = 3) FROM docs LIMIT 5;",
            "QUERY RELEVANCE FEEDBACK TARGET TEXT 'x' FEEDBACK ((TEXT 'y', 1.0)) STRATEGY NAIVE (a = 1, b = 2, c = 3, d = 4) FROM docs LIMIT 5;",
            // F-5d rerank_input is TEXT | VECTOR | POINT only.
            "QUERY RERANK 'x' MODEL 'm' FROM docs LIMIT 5;",
            "QUERY RERANK IMAGE 'a.png' MODEL 'm' FROM docs LIMIT 5;",
            // F-5h lowercase raw prefix only.
            "QUERY R'a\\nb' FROM docs LIMIT 5;",
            // F-5i dotted segments start with a letter or `_`.
            "QUERY TEXT 'x' FROM docs WHERE a.$b = 1 LIMIT 5;",
            // F-14 COUNT config is exactly `exact = true|false`.
            "COUNT FROM docs WITH (exact = 5);",
            "COUNT FROM docs WITH (exact = 'yes');",
            "COUNT FROM docs WITH (exact = true, foo = 1);",
            "COUNT FROM docs WITH (foo = 1);",
            // F-15 CREATE SHARD KEY config is exactly positive-integer
            // shards_number / replication_factor.
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 0);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = -1);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2.0);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (replication_factor = 'three');",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2, foo = 1);",
        ];
        let both_accept = [
            // F-2 `''''` is the SQL-escaped one-apostrophe string.
            "UPSERT INTO docs VALUES {id: 1, text: ''''};",
            // F-1 triple-quoted strings preserve contents verbatim.
            "UPSERT INTO docs VALUES {id: 1, text: '''a\\nb'''};",
            "UPSERT INTO docs VALUES {id: 1, text: '''it''s'''};",
            // F-5a valid grammar order.
            "COUNT FROM docs WHERE active = true SHARD 'x' WITH (exact = true);",
            // F-5e embed directive kinds match single_embedding_spec.
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'} EMBED text INTO v USING MULTI MODEL 'm';",
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'} EMBED text INTO v USING IMAGE MODEL 'm';",
            // F-5g consistency accepts non-negative factors and keywords.
            "QUERY TEXT 'x' FROM docs PARAMS (consistency = 0) LIMIT 5;",
            "QUERY TEXT 'x' FROM docs PARAMS (consistency = majority) LIMIT 5;",
            // F-14/F-15 strict config shapes (case-insensitive keys; quoted
            // keys stay aligned with the reference parser's config blocks).
            "COUNT FROM docs WITH (exact = true);",
            "COUNT FROM docs WITH (Exact = FALSE);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (replication_factor = 3);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (SHARDS_NUMBER = 4, REPLICATION_FACTOR = 2);",
            "CREATE SHARD KEY 'a' ON COLLECTION docs WITH ('shards_number' = 2);",
            // MAX / MIN fold n ≥ 1 operands: a single operand is valid in both
            // the grammar and the runtime; the empty list is rejected by both.
            "QUERY FORMULA MAX($score) DEFAULTS (score = 0.0) FROM docs LIMIT 5;",
            "QUERY FORMULA MIN($score) DEFAULTS (score = 0.0) FROM docs LIMIT 5;",
            "QUERY FORMULA MAX($score, 1.0, 2.0) DEFAULTS (score = 0.0) FROM docs LIMIT 5;",
        ];

        for source in both_reject {
            assert!(
                QqlGrammar::parse(Rule::single_statement, source).is_err(),
                "grammar should reject: {source}"
            );
            assert!(
                QqlParser::parse_all(source).is_err(),
                "runtime should reject: {source}"
            );
        }
        for source in both_accept {
            assert!(
                QqlGrammar::parse(Rule::single_statement, source).is_ok(),
                "grammar should accept: {source}"
            );
            assert!(
                QqlParser::parse_all(source).is_ok(),
                "runtime should accept: {source}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annotated_invalid_cases() {
        let source = "\
-- file header
-- @case missing-from
-- @error QQL-VALIDATION-FROM
QUERY 'x';

-- @case empty-statement
;
";
        assert_eq!(
            invalid_cases(source).unwrap(),
            vec![
                InvalidCase {
                    name: "missing-from".to_owned(),
                    expected_error: Some("QQL-VALIDATION-FROM".to_owned()),
                    source: "QUERY 'x';".to_owned(),
                },
                InvalidCase {
                    name: "empty-statement".to_owned(),
                    expected_error: None,
                    source: ";".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn rejects_unmarked_source() {
        assert!(invalid_cases("QUERY 'x';").is_err());
    }

    #[test]
    fn canonical_ast_is_versioned() {
        let statements = Parser::parse_all("SHOW COLLECTIONS;").unwrap();
        let ast = canonical_ast(&statements).unwrap();
        assert_eq!(ast["schema"], AST_SCHEMA);
        assert!(ast["statements"].is_array());
    }

    #[test]
    fn canonical_ast_rounds_floating_point_noise() {
        let statements =
            Parser::parse_all("QUERY VECTOR [0.1, 0.2, 0.3] FROM docs LIMIT 3;").unwrap();
        let ast = canonical_ast(&statements).unwrap();
        let dense =
            &ast["statements"][0]["Query"]["expression"]["Nearest"]["input"]["Vector"]["Dense"];
        assert_eq!(dense, &serde_json::json!([0.1, 0.2, 0.3]));
    }
}
