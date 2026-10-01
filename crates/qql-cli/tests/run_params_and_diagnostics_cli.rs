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
fn rerank_with_text_requires_embedder() {
    let out = qql(&[
        "run",
        "QUERY RERANK TEXT 'travel tips' MODEL 'colbert-v2' FROM stays USING colbert \
         PREFETCH (QUERY TEXT 'travel tips' MODEL 'e5' FROM stays USING dense LIMIT 50) LIMIT 10;",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("QQL-EMBEDDING-UNAVAILABLE"),
        "RERANK must demand an embedder before any network call, got: {stderr}"
    );
}

#[test]
fn batch_member_text_requires_embedder() {
    let out = qql(&[
        "run",
        "BATCH { QUERY TEXT 'x' FROM stays USING dense LIMIT 1; }",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("QQL-EMBEDDING-UNAVAILABLE"),
        "BATCH members must be walked for embedder needs, got: {stderr}"
    );
}

#[test]
fn precomputed_vector_param_does_not_require_an_embedder() {
    let json_path = temp_json("qvec", &serde_json::json!({"qvec": [0.1, 0.2]}).to_string());
    let out = qql(&[
        "run",
        "--params-file",
        json_path.to_str().unwrap(),
        "QUERY :qvec FROM docs LIMIT 1;",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("QQL-EMBEDDING-UNAVAILABLE"),
        "a bound vector param must not demand an embedder, got: {stderr}"
    );
    // No backend is running in tests: the run reaches dispatch and fails there.
    assert!(!out.status.success());
}

#[test]
fn run_file_defaults_to_human_summary_and_respects_json_and_quiet() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("qql-run-file-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let script = dir.join("empty.qql");
    std::fs::write(&script, "-- only a comment\n").expect("write script");

    let human = qql(&["run", script.to_str().unwrap()]);
    assert!(human.status.success(), "empty script run must succeed");
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("Ran script") && !stdout.contains("\"succeeded\""),
        "human mode must print the summary, got: {stdout}"
    );

    let json = qql(&["run", "--json", script.to_str().unwrap()]);
    assert!(json.status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&json.stdout)).expect("json report");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["succeeded"], 0);
    assert_eq!(parsed["failed"], 0);

    let quiet = qql(&["run", "--quiet", script.to_str().unwrap()]);
    assert!(quiet.status.success());
    assert!(
        quiet.stdout.is_empty(),
        "quiet mode must print nothing, got: {}",
        String::from_utf8_lossy(&quiet.stdout)
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
fn test_version_command_reports_valid_json_and_edition() {
    let out = qql(&["version"]);
    assert!(out.status.success(), "qql version should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let val: serde_json::Value =
        serde_json::from_str(&stdout).expect("version output must be valid json");
    assert_eq!(val["ok"], true);
    assert_eq!(val["command"], "version");
    assert!(val["version"].is_string());
    assert!(val["edition"].is_string());
    let edition = val["edition"].as_str().unwrap();
    assert!(
        edition == "standard" || edition == "full" || edition == "custom",
        "unexpected edition: {edition}"
    );
    let message = val["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("({edition})")),
        "message should contain edition in parens, got: {message}"
    );
}
