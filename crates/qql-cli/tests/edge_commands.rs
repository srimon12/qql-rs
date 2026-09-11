//! `qql edge optimize` / `qql edge bootstrap` CLI e2e (edge builds only).
//!
//! Covers the structured `--json` failure shape and the optimize indexing-lag
//! reporting. Collections are seeded through `EdgeQdrant` directly so no
//! embedding model is loaded.

#![cfg(feature = "edge")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use qql::client::QdrantOps;
use qql_edge::EdgeQdrant;
use qql_plan::{PlannedOperation, plan};

fn qql() -> Command {
    Command::new(env!("CARGO_BIN_EXE_qql"))
}

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "qql-edge-cmd-it-{}-{unique}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn plan_one(sql: &str) -> PlannedOperation {
    let statement = qql_core::parser::Parser::parse(sql)
        .unwrap_or_else(|error| panic!("parse '{sql}': {error}"));
    plan(&statement).unwrap_or_else(|error| panic!("plan '{sql}': {error}"))
}

fn run(args: &[&str], home: &Path, data: &Path) -> Output {
    qql()
        .env("HOME", home)
        .env("QQL_EDGE_DATA_DIR", data)
        .args(args)
        .output()
        .expect("spawn qql")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

async fn seed(data: &Path, collection: &str, points: usize, indexing_threshold_kb: Option<u64>) {
    let backend = EdgeQdrant::new(data, false);
    let optimizers = indexing_threshold_kb
        .map(|kb| format!(" WITH OPTIMIZERS (indexing_threshold = {kb})"))
        .unwrap_or_default();
    backend
        .execute_planned(&plan_one(&format!(
            "CREATE COLLECTION {collection} (dense VECTOR(1, COSINE)){optimizers}"
        )))
        .await
        .expect("create collection");
    let values = (1..=points)
        .map(|id| format!("{{id: {id}, vector: {{dense: [1.0]}}}}"))
        .collect::<Vec<_>>()
        .join(", ");
    backend
        .execute_planned(&plan_one(&format!(
            "UPSERT INTO {collection} VALUES {values}"
        )))
        .await
        .expect("upsert points");
    backend.close().await.expect("close backend");
}

#[tokio::test]
async fn optimize_reports_indexing_lag_as_warning() {
    let home = temp_dir("opt-lag");
    let data = home.join("data");
    // Default indexing_threshold: the segment stays below it and remains
    // brute-force even after the optimizer ran.
    seed(&data, "docs", 300, None).await;

    let output = run(&["edge", "optimize", "docs", "--json"], &home, &data);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = stdout_json(&output);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["operation"], "edge-optimize");
    assert_eq!(report["collection"], "docs");
    assert_eq!(report["status"], "warn", "{report}");
    assert_eq!(report["indexing_lag"], true, "{report}");
    let message = report["message"].as_str().expect("message");
    assert!(message.contains("indexing still lags"), "{message}");
    assert!(
        message.contains("of 300 vectors indexed"),
        "lag counts must be accurate: {message}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn optimize_reports_ok_when_fully_indexed() {
    let home = temp_dir("opt-ok");
    let data = home.join("data");
    // 1 KB threshold: 300 one-dimensional vectors exceed it, so the indexing
    // optimizer builds HNSW for the whole segment.
    seed(&data, "docs", 300, Some(1)).await;

    let output = run(&["edge", "optimize", "docs", "--json"], &home, &data);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = stdout_json(&output);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["status"], "ok", "{report}");
    assert_eq!(report["indexing_lag"], false, "{report}");
    assert_eq!(report["after"]["indexed"], report["after"]["points"]);
    let message = report["message"].as_str().expect("message");
    assert!(!message.contains("lags"), "{message}");

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn optimize_json_error_is_structured() {
    let home = temp_dir("opt-err");
    let data = home.join("data");
    std::fs::create_dir_all(&data).expect("create data dir");

    let output = run(&["edge", "optimize", "missing", "--json"], &home, &data);
    assert!(!output.status.success(), "missing collection must fail");
    let report = stdout_json(&output);
    assert_eq!(report["ok"], false, "{report}");
    assert_eq!(report["operation"], "edge-optimize");
    assert_eq!(report["collection"], "missing");
    assert!(
        report["message"]
            .as_str()
            .expect("message")
            .contains("QQL-EDGE-COLLECTION-NOT-FOUND"),
        "{report}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn bootstrap_json_error_is_structured() {
    let home = temp_dir("boot-err");
    let data = home.join("data");

    // Discard port: the shard-discovery request is refused, so the snapshot
    // never downloads and no collection directory is created.
    let output = run(
        &[
            "edge",
            "bootstrap",
            "docs",
            "--from",
            "http://127.0.0.1:9",
            "--json",
        ],
        &home,
        &data,
    );
    assert!(!output.status.success(), "unreachable source must fail");
    let report = stdout_json(&output);
    assert_eq!(report["ok"], false, "{report}");
    assert_eq!(report["operation"], "edge-bootstrap");
    assert_eq!(report["collection"], "docs");
    assert!(
        report["message"]
            .as_str()
            .expect("message")
            .contains("QQL-TRANSPORT"),
        "{report}"
    );
    assert!(
        !data.join("docs").exists(),
        "failed bootstrap must not create the collection"
    );

    let _ = std::fs::remove_dir_all(&home);
}
