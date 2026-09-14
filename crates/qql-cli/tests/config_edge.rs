//! `qql config edge` merge semantics (e2e, isolated `$HOME`).
//!
//! A config update must patch the persisted `edge.json` in place: keys it does
//! not set — known siblings and unknown/future keys alike — survive.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn qql() -> Command {
    Command::new(env!("CARGO_BIN_EXE_qql"))
}

fn temp_home(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "qql-config-edge-it-{}-{unique}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".qql")).expect("create .qql");
    home
}

#[test]
fn config_update_preserves_sibling_and_unknown_keys() {
    let home = temp_home("merge");
    let config = home.join(".qql").join("edge.json");
    std::fs::write(
        &config,
        serde_json::to_string_pretty(&serde_json::json!({
            "wal_segment_mb": 8,
            "embedder": "http",
            "embed_url": "http://127.0.0.1:9/v1/embeddings",
            "embed_model": "custom-model",
            "future_knob": { "nested": true },
        }))
        .expect("serialize seed"),
    )
    .expect("seed edge.json");

    let output = qql()
        .env("HOME", &home)
        .args(["config", "edge", "--wal-segment-mb", "4"])
        .output()
        .expect("spawn qql");
    assert!(
        output.status.success(),
        "config update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).expect("read merged"))
            .expect("parse merged");
    // The patched key is updated...
    assert_eq!(merged["wal_segment_mb"], serde_json::json!(4));
    // ...and every sibling survives, known or not.
    assert_eq!(merged["embedder"], serde_json::json!("http"));
    assert_eq!(
        merged["embed_url"],
        serde_json::json!("http://127.0.0.1:9/v1/embeddings")
    );
    assert_eq!(merged["embed_model"], serde_json::json!("custom-model"));
    assert_eq!(merged["future_knob"], serde_json::json!({ "nested": true }));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&config)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "edge.json must stay owner-only");
    }

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn config_update_validates_the_merged_state() {
    let home = temp_home("validate");
    let config = home.join(".qql").join("edge.json");
    std::fs::write(
        &config,
        r#"{"embedder": "http", "embed_url": "http://x/v1"}"#,
    )
    .expect("seed edge.json");

    // The persisted embed_url satisfies the http requirement even though this
    // invocation does not repeat it.
    let output = qql()
        .env("HOME", &home)
        .args(["config", "edge", "--wal-segment-mb", "4"])
        .output()
        .expect("spawn qql");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A zero WAL knob fails closed on the merged state.
    let output = qql()
        .env("HOME", &home)
        .args(["config", "edge", "--wal-segment-mb", "0"])
        .output()
        .expect("spawn qql");
    assert!(!output.status.success(), "wal 0 must fail closed");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("greater than zero"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&home);
}
