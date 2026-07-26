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
     check     Parse every valid case, reject every invalid case, and compare AST snapshots.\n\
     generate  Validate all cases, then regenerate fixtures/expected/*.json."
}

fn check(spec_dir: &Path, require_expected: bool) -> Result<(), Box<dyn Error>> {
    let report = inspect(spec_dir, require_expected)?;
    if report.failures.is_empty() {
        println!(
            "conformant: {} valid files ({} statements), {} invalid cases, {} AST snapshots",
            report.valid_files,
            report.valid_statements,
            report.invalid_cases,
            report.expected_files
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
    fs::create_dir_all(&expected_dir)?;

    let mut written = 0;
    for fixture in qql_files(&valid_dir)? {
        let source = fs::read_to_string(&fixture)?;
        let statements =
            parse_and_validate(&source).map_err(|error| format_parse_failure(&fixture, &error))?;
        let json = serde_json::to_string_pretty(&canonical_ast(&statements)?)?;
        let output = expected_dir.join(
            fixture
                .file_stem()
                .expect("fixture path has a file stem")
                .to_string_lossy()
                .to_string()
                + ".json",
        );
        write_atomic(&output, &(json + "\n"))?;
        written += 1;
    }

    println!(
        "generated {written} AST snapshot(s) in {}",
        expected_dir.display()
    );
    Ok(())
}

#[derive(Default)]
struct Report {
    valid_files: usize,
    valid_statements: usize,
    invalid_cases: usize,
    expected_files: usize,
    failures: Vec<String>,
}

fn inspect(spec_dir: &Path, require_expected: bool) -> Result<Report, Box<dyn Error>> {
    let valid_dir = spec_dir.join("fixtures/valid");
    let invalid_dir = spec_dir.join("fixtures/invalid");
    let expected_dir = spec_dir.join("fixtures/expected");
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
                    compare_expected(&fixture, &expected_dir, &statements, &mut report)?;
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
                            if let Some(expected) = case.expected_error.as_deref() {
                                if error.code != expected {
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
    expected_dir: &Path,
    statements: &[qql_core::ast::Stmt],
    report: &mut Report,
) -> Result<(), Box<dyn Error>> {
    let expected = expected_dir.join(
        fixture
            .file_stem()
            .expect("fixture path has a file stem")
            .to_string_lossy()
            .to_string()
            + ".json",
    );
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

fn qql_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("qql")))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
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
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)?;
    Ok(())
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
