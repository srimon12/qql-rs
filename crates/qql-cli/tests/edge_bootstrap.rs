//! Live snapshot-bootstrap integration test (ignored by default).
//!
//! Requires a Qdrant server (default `http://localhost:6333`, override with
//! `QDRANT_URL`) and the `edge` feature. Run with:
//!
//! ```sh
//! cargo test -p qql-cli --features edge --test edge_bootstrap -- --ignored --nocapture
//! ```
//!
//! The test creates a uniquely named collection, seeds two points over REST via
//! the CLI itself, bootstraps a local edge shard from the remote shard
//! snapshot, checks the fail-closed overwrite guard and `--force`, then drops
//! the collection and the temp data dir.

#![cfg(feature = "edge")]

use std::process::Command;

fn qql() -> Command {
    Command::new(env!("CARGO_BIN_EXE_qql"))
}

fn base_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string())
}

fn run(mut command: Command) -> (bool, String) {
    let output = command.output().expect("spawn qql");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
#[ignore = "requires a live Qdrant server (QDRANT_URL, default http://localhost:6333)"]
fn edge_bootstrap_seeds_from_remote_shard_snapshot() {
    let url = base_url();
    let collection = format!("qql_boot_it_{}", std::process::id());
    let data_dir = std::env::temp_dir().join(format!("qql-boot-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);

    // Seed the remote collection with literal vectors (no embedder needed).
    let (ok, out) = run({
        let mut command = qql();
        command.args([
            "--url",
            &url,
            "exec",
            &format!("CREATE COLLECTION {collection} (dense VECTOR(4, COSINE));"),
        ]);
        command
    });
    assert!(ok, "create collection failed: {out}");

    let (ok, out) = run({
        let mut command = qql();
        command.args([
            "--url",
            &url,
            "exec",
            &format!(
                "UPSERT INTO {collection} VALUES \
                 {{id: 1, vector: {{dense: [0.1, 0.2, 0.3, 0.4]}}}}, \
                 {{id: 2, vector: {{dense: [0.4, 0.3, 0.2, 0.1]}}}};"
            ),
        ]);
        command
    });
    assert!(ok, "seed upsert failed: {out}");

    // First bootstrap: single-shard collection auto-discovers shard 0.
    let (ok, out) = run({
        let mut command = qql();
        command.env("QQL_EDGE_DATA_DIR", &data_dir).args([
            "edge",
            "bootstrap",
            &collection,
            "--from",
            &url,
            "--json",
        ]);
        command
    });
    assert!(ok, "bootstrap failed: {out}");
    let report: serde_json::Value = serde_json::from_str(out.trim()).expect("bootstrap JSON");
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["shard_id"], 0, "{report}");
    assert_eq!(report["points_count"], 2, "{report}");

    // Existing local collection: fails closed without --force, untouched.
    let (ok, out) = run({
        let mut command = qql();
        command.env("QQL_EDGE_DATA_DIR", &data_dir).args([
            "edge",
            "bootstrap",
            &collection,
            "--from",
            &url,
        ]);
        command
    });
    assert!(!ok, "second bootstrap must fail without --force: {out}");
    assert!(out.contains("already exists"), "{out}");

    // --force replaces after the snapshot verifies.
    let (ok, out) = run({
        let mut command = qql();
        command.env("QQL_EDGE_DATA_DIR", &data_dir).args([
            "edge",
            "bootstrap",
            &collection,
            "--from",
            &url,
            "--force",
        ]);
        command
    });
    assert!(ok, "forced bootstrap failed: {out}");

    // The bootstrapped shard loads and optimizes locally.
    let (ok, out) = run({
        let mut command = qql();
        command
            .env("QQL_EDGE_DATA_DIR", &data_dir)
            .args(["edge", "optimize", &collection]);
        command
    });
    assert!(ok, "optimize on the bootstrapped shard failed: {out}");

    // Cleanup: remote collection and local data dir.
    let _ = run({
        let mut command = qql();
        command.args([
            "--url",
            &url,
            "exec",
            &format!("DROP COLLECTION {collection};"),
        ]);
        command
    });
    let _ = std::fs::remove_dir_all(&data_dir);
}
