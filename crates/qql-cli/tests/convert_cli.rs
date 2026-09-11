//! End-to-end `qql convert` tests at the process boundary.
//!
//! Pins the JSONL fallback for `qql record --out` captures, the `--collection`
//! flag for bare bodies, statement terminators, and line-numbered failures.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_jsonl(tag: &str, contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("qql-convert-it-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{tag}.jsonl"));
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
fn bare_body_without_collection_renders_unknown() {
    let path = temp_jsonl("bare", "{\"ids\": [1]}\n");
    let out = qql(&["convert", path.to_str().expect("utf8 path")]);

    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("FROM unknown"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
