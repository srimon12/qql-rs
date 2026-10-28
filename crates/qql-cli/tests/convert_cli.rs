//! End-to-end `qql convert` tests at the process boundary.
//!
//! Pins the JSONL fallback for `qql record --out` captures, the `--collection`
//! flag for bare bodies, statement terminators, and line-numbered failures.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "qql-convert-it-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn temp_file(tag: &str, name: &str, contents: &str) -> PathBuf {
    let path = temp_dir(tag).join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn temp_jsonl(tag: &str, contents: &str) -> PathBuf {
    temp_file(tag, &format!("{tag}.jsonl"), contents)
}

fn qql(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(args)
        .output()
        .expect("run qql")
}

#[test]
fn jsonl_capture_converts_statement_per_line() {
    let capture = concat!(
        r#"{"method":"POST","path":"/collections/docs/points/count","body":{"exact":true}}"#,
        "\n",
        r#"{"ids":[1,"point-2"]}"#,
        "\n",
    );
    let path = temp_jsonl("capture", capture);
    let out = qql(&[
        "convert",
        "--collection",
        "docs",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("COUNT FROM docs"), "stdout: {stdout}");
    assert!(
        stdout.contains("QUERY POINTS (1, 'point-2') FROM docs;"),
        "stdout: {stdout}"
    );
}

#[test]
fn bad_capture_line_is_reported_with_its_number() {
    let capture = concat!(
        r#"{"method":"POST","path":"/collections/docs/points/count","body":{}}"#,
        "\n",
        r#"{"method":"POST","path":"/collections/docs/aliases","body":{}}"#,
        "\n",
    );
    let path = temp_jsonl("bad-line", capture);
    let out = qql(&["convert", path.to_str().expect("utf8 path")]);

    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("line 2"), "stderr: {stderr}");
}

#[test]
fn bare_body_without_collection_fails() {
    let path = temp_jsonl("bare", "{\"ids\": [1]}\n");
    let out = qql(&["convert", path.to_str().expect("utf8 path")]);

    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("requires a collection"), "stderr: {stderr}");
}

#[test]
fn curl_vars_substitute_end_to_end() {
    let path = temp_file(
        "vars",
        "request.curl",
        "curl -X POST http://localhost:6333/collections/$TENANT/points/count -d '{\"exact\": true}'",
    );
    let out = qql(&[
        "convert",
        "--var",
        "TENANT=first",
        "--var",
        "TENANT=acme",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("COUNT FROM acme WITH (exact = true);"),
        "stdout: {stdout}"
    );
}

#[test]
fn curl_at_file_body_resolves_end_to_end() {
    let dir = temp_dir("at-file");
    let body = dir.join("body.json");
    std::fs::write(&body, r#"{"exact": true}"#).expect("write body");
    let request = dir.join("request.curl");
    std::fs::write(
        &request,
        format!(
            "curl -X POST http://localhost:6333/collections/docs/points/count -d @{}",
            body.display()
                .to_string()
                .replace('\\', "/")
                .to_string()
                .replace('\\', "/")
        ),
    )
    .expect("write curl");

    let out = qql(&["convert", request.to_str().expect("utf8 path")]);

    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("COUNT FROM docs WITH (exact = true);"),
        "stdout: {stdout}"
    );
}

#[test]
fn at_file_contents_are_not_var_expanded() {
    let dir = temp_dir("at-file-no-vars");
    let body = dir.join("body.json");
    std::fs::write(
        &body,
        r#"{"points":[{"id":1,"vector":[0.1,0.2],"payload":{"tenant":"$TENANT"}}]}"#,
    )
    .expect("write body");
    let request = dir.join("request.curl");
    std::fs::write(
        &request,
        format!(
            "curl -X PUT http://localhost:6333/collections/docs/points -d @{}",
            body.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write curl");

    let out = qql(&[
        "convert",
        "--var",
        "TENANT=acme",
        request.to_str().expect("utf8 path"),
    ]);

    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("$TENANT") && !stdout.contains("acme"),
        "file bodies must not be var-expanded, got: {stdout}"
    );
}

#[test]
fn var_without_equals_is_rejected() {
    let path = temp_jsonl("var-bad", "{\"ids\": [1]}\n");
    let out = qql(&[
        "convert",
        "--collection",
        "docs",
        "--var",
        "TENANT",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("KEY=VALUE"), "stderr: {stderr}");
}

#[test]
fn unsafe_var_value_is_rejected_and_names_the_variable() {
    let path = temp_file(
        "unsafe-var",
        "request.curl",
        "curl -X POST http://localhost:6333/collections/docs/points/count -d '{\"filter\": {\"must\": [{\"key\": \"tenant\", \"match\": {\"value\": \"$TENANT\"}}]}}'",
    );
    let out = qql(&[
        "convert",
        "--var",
        "TENANT=ac\"me",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(!out.status.success(), "quote must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--var TENANT"), "stderr: {stderr}");
    assert!(
        !stderr.contains("invalid JSON"),
        "the value must fail before parsing, got: {stderr}"
    );
}

#[test]
fn var_value_cannot_inject_a_second_filter() {
    let path = temp_file(
        "inject-var",
        "request.curl",
        "curl -X POST http://localhost:6333/collections/docs/points/count -d '{\"filter\": {\"must\": [{\"key\": \"tenant\", \"match\": {\"value\": \"$TENANT\"}}]}}'",
    );
    let out = qql(&[
        "convert",
        "--var",
        "TENANT=acme\"}},{\"key\":\"role\",\"match\":{\"value\":\"admin",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(!out.status.success(), "injection must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--var TENANT"), "stderr: {stderr}");
}

#[test]
fn at_file_directory_is_rejected() {
    let dir = temp_dir("at-dir");
    let request = dir.join("request.curl");
    std::fs::write(
        &request,
        format!(
            "curl -X POST http://localhost:6333/collections/docs/points/count -d @{}",
            dir.display()
                .to_string()
                .replace('\\', "/")
                .to_string()
                .replace('\\', "/")
        ),
    )
    .expect("write curl");

    let out = qql(&["convert", request.to_str().expect("utf8 path")]);

    assert!(!out.status.success(), "a directory is not a body");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a regular file"), "stderr: {stderr}");
}

#[test]
fn missing_at_file_reports_the_path_once() {
    let request = temp_file(
        "missing-at",
        "request.curl",
        "curl -X POST http://localhost:6333/collections/docs/points/count -d @nope.json",
    );
    let out = qql(&["convert", request.to_str().expect("utf8 path")]);

    assert!(!out.status.success(), "missing file must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.matches("nope.json").count(),
        1,
        "the path must appear once, got: {stderr}"
    );
}
