//! End-to-end `qql lint` tests.

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
fn lint_reports_redundant_payload() {
    let with_payload = "QUERY [0.1, 0.2] FROM docs WITH PAYLOAD true LIMIT 5;\n";
    let path = temp_file("payload", with_payload);
    let out = qql(&["lint", path.to_str().unwrap()]);
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("QQL-IDIOM-REDUNDANT-PAYLOAD"));
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
fn lint_json_output() {
    let code = "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n";
    let path = temp_file("json_test", code);
    let out = qql(&["lint", path.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert!(parsed.is_array());
    assert_eq!(parsed[0]["valid"], true);
}
