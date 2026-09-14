//! End-to-end `qql lint` tests (all offline — no backend required).

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_file(tag: &str, contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("qql-lint-it-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{tag}.qql"));
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn qql(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(args)
        .output()
        .expect("run qql")
}

#[test]
fn lint_passes_on_valid_canonical_input() {
    let clean = "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n";
    let path = temp_file("clean", clean);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn lint_fails_on_syntax_error() {
    let bad = "QUERY FROM docs LIMIT 5;\n";
    let path = temp_file("bad_syntax", bad);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(combined.contains("QQL-PARSE-"));
}

#[test]
fn lint_reports_redundant_payload_on_stderr_with_precise_span() {
    let with_payload = "QUERY [0.1, 0.2] FROM docs WITH PAYLOAD true LIMIT 5;\n";
    let path = temp_file("payload", with_payload);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(!out.status.success());
    // Human diagnostics go to stderr; stdout stays clean for pipes/JSON.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stdout.contains("QQL-IDIOM-REDUNDANT-PAYLOAD"),
        "diagnostics must not pollute stdout"
    );
    assert!(stderr.contains("QQL-IDIOM-REDUNDANT-PAYLOAD"));
    // The diagnostic points at the clause (`WITH` starts at byte 27 here),
    // not at the start of the statement.
    let json_out = qql(&["lint", path.to_str().unwrap(), "--json"]);
    let parsed: serde_json::Value = serde_json::from_slice(&json_out.stdout).expect("valid json");
    assert_eq!(parsed["ok"], false);
    let diag = &parsed["files"][0]["diagnostics"][0];
    assert_eq!(diag["code"], "QQL-IDIOM-REDUNDANT-PAYLOAD");
    assert_eq!(diag["span_start"], 27);
    assert_eq!(diag["column"], 28);
}

#[test]
fn lint_fix_cleans_redundant_payload_and_duplicate_wait() {
    let buggy = "UPSERT INTO docs VALUES {id: 1} WAIT true WAIT true;\n";
    let path = temp_file("buggy", buggy);
    let out = qql(&["lint", "--fix", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fixed_content = std::fs::read_to_string(&path).unwrap();
    assert!(!fixed_content.contains("WAIT true WAIT true"));
    assert!(fixed_content.contains("WAIT true"));

    // Re-linting should now pass cleanly
    let check = qql(&["lint", path.to_str().unwrap()]);
    assert!(check.status.success());
}

#[test]
fn lint_fix_keeps_trailing_wait_on_conflict() {
    let buggy = "UPSERT INTO docs VALUES {id: 1} WAIT true WAIT false;\n";
    let path = temp_file("wait_conflict", buggy);
    let out = qql(&["lint", "--fix", path.to_str().unwrap()]);
    assert!(out.status.success());
    let fixed_content = std::fs::read_to_string(&path).unwrap();
    assert!(fixed_content.contains("WAIT false"));
    assert!(!fixed_content.contains("WAIT true WAIT false"));
}

#[test]
fn lint_fix_never_touches_string_literals() {
    // The payload words live inside a text literal: no diagnostic, no rewrite.
    let tricky = "QUERY 'WITH PAYLOAD TRUE' FROM docs LIMIT 5;\n";
    let path = temp_file("str_literal", tricky);
    let out = qql(&["lint", "--fix", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), tricky);

    // A WAIT inside a string literal is not a clause: the real trailing WAIT
    // must survive while the string stays byte-identical.
    let tricky_wait = "UPSERT INTO docs VALUES {id: 1, text: 'WAIT true'} WAIT true;\n";
    let path = temp_file("str_wait", tricky_wait);
    let out = qql(&["lint", "--fix", path.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), tricky_wait);
}

#[test]
fn lint_missing_qql_path_is_not_misread_as_query() {
    let out = qql(&["lint", "definitely-missing-file.qql"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no such file"), "stderr was: {stderr}");
}

#[test]
fn lint_stdin_fix_still_fails_on_syntax_errors() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(["lint", "--fix"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qql");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"QUERY FROM docs LIMIT 5;\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn lint_binds_params_from_flag_and_header() {
    let param = "QUERY :q FROM docs USING dense LIMIT 5;\n";
    let path = temp_file("param", param);
    // Unbound: fails with a hint telling the user how to bind.
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("QQL-BIND-MISSING-PARAM"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Bound via --param: passes.
    let out = qql(&["lint", path.to_str().unwrap(), "-p", "q=hello"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Bound via the `-- qql-params:` header: passes with no flags.
    let header = "-- qql-params: {\"q\": \"hello\"}\nQUERY :q FROM docs USING dense LIMIT 5;\n";
    let path = temp_file("param_header", header);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn lint_flags_scroll_redundant_payload() {
    let scroll = "SCROLL FROM docs WITH PAYLOAD true LIMIT 5;\n";
    let path = temp_file("scroll_payload", scroll);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("QQL-IDIOM-REDUNDANT-PAYLOAD"));
    let out = qql(&["lint", "--fix", path.to_str().unwrap()]);
    assert!(out.status.success());
    let fixed = std::fs::read_to_string(&path).unwrap();
    assert!(!fixed.contains("PAYLOAD true"));
}

#[test]
fn lint_json_output() {
    let code = "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n";
    let path = temp_file("json_test", code);
    let out = qql(&["lint", path.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["files"][0]["valid"], true);
    assert!(parsed.get("content").is_none());
}

#[test]
fn lint_json_envelope_is_stable_across_modes() {
    // Stdin mode emits the same envelope shape as file mode.
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(["lint", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qql");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"QUERY [0.1] FROM docs LIMIT 1;\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["files"][0]["file"], "<stdin>");

    // Inline-string fix mode adds `content` with the fixed text.
    let out = qql(&[
        "lint",
        "QUERY [0.1] FROM docs WITH PAYLOAD true LIMIT 1;",
        "--fix",
        "--json",
    ]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["files"][0]["fixed"], true);
    assert!(
        !parsed["content"]
            .as_str()
            .expect("content is text")
            .contains("PAYLOAD true")
    );
}

#[test]
fn lint_non_wait_duplicate_clause_is_not_marked_fixable() {
    let facet_dup = "FACET category FROM docs LIMIT 5 LIMIT 10;\n";
    let path = temp_file("facet_dup", facet_dup);
    let out = qql(&["lint", path.to_str().unwrap(), "--json"]);
    assert!(!out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let diag = &parsed["files"][0]["diagnostics"][0];
    assert_eq!(diag["code"], "QQL-PARSE-DUPLICATE-CLAUSE");
    assert_eq!(diag["fixable"], false);
}
