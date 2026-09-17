//! End-to-end tests for CLI embedder diagnostics and parameter ergonomics.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_json(tag: &str, contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("qql-params-it-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{tag}.json"));
    std::fs::write(&path, contents).expect("write json fixture");
    path
}

fn qql(args: &[&str]) -> std::process::Output {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let home = std::env::temp_dir().join(format!("qql-test-home-{nanos}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&home);
    Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(args)
        .env("HOME", &home)
        .env_remove("EMBED_URL")
        .env_remove("EMBED_TYPE")
        .env_remove("QQL_FASTEMBED")
        .output()
        .expect("run qql")
}

#[test]
fn missing_embedder_returns_qql_embedding_unavailable() {
    let out = qql(&[
        "run",
        "QUERY TEXT 'apartment' FROM stays USING dense LIMIT 5;",
    ]);
    assert!(!out.status.success(), "run should fail without embedder");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("QQL-EMBEDDING-UNAVAILABLE"),
        "expected QQL-EMBEDDING-UNAVAILABLE error code, got: {stderr}"
    );
    assert!(
        stderr.contains("--params-file"),
        "expected remediation guidance mentioning --params-file, got: {stderr}"
    );
}

#[test]
fn explain_with_high_dimensional_vector_params_file() {
    // 384-dimensional vector
    let vec_384: Vec<f64> = (0..384).map(|i| (i as f64) * 0.001).collect();
    let json_content = serde_json::json!({
        "qvec": vec_384
    })
    .to_string();

    let json_path = temp_json("vec384", &json_content);

    let out = qql(&[
        "explain",
        "--params-file",
        json_path.to_str().unwrap(),
        "QUERY :qvec FROM docs LIMIT 5;",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Query Nearest") || stdout.contains("docs"),
        "expected plan output, got: {stdout}"
    );
}

#[test]
fn repl_help_displays_params_file_flag() {
    let out = qql(&["repl", "--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--params-file"));
    assert!(stdout.contains("--param"));
}

#[test]
fn record_command_delegates_or_emits_guidance() {
    let out = qql(&["record", "--listen", "127.0.0.1:6339"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Either qql-record executes or guidance is printed
    assert!(
        stderr.contains("qql-record") || out.status.success(),
        "expected reference to qql-record binary, got: {stderr}"
    );
}
