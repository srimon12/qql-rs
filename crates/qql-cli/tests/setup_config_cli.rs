//! End-to-end `qql config` and `qql setup` CLI tests with isolated HOME dirs.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_home(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "qql-test-home-{tag}-{nanos}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn qql(args: &[&str], home: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run qql")
}

#[test]
fn config_path_prints_path() {
    let home = temp_home("path");
    let out = qql(&["config", "path"], &home);
    assert!(out.status.success());
    let path = String::from_utf8_lossy(&out.stdout);
    assert!(path.trim().ends_with("config.json"));
    assert!(path.trim().contains(home.to_str().unwrap()));
}

#[test]
fn config_set_and_get() {
    let home = temp_home("set_get");
    let test_url = "http://test-cluster.example.com:6333";
    let set_out = qql(&["config", "set", "url", test_url], &home);
    assert!(set_out.status.success());

    let get_out = qql(&["config", "get", "url"], &home);
    assert!(get_out.status.success());
    assert_eq!(String::from_utf8_lossy(&get_out.stdout).trim(), test_url);
}

#[test]
fn config_show_json() {
    let home = temp_home("show_json");
    let out = qql(&["config", "show", "--json"], &home);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert!(parsed.get("config").is_some());
    assert!(parsed.get("path").is_some());
}

#[test]
fn setup_non_interactive_saves_config() {
    let home = temp_home("setup");
    let test_url = "http://setup-test.example.com:6333";
    let out = qql(&["setup", "--url", test_url, "--non-interactive"], &home);
    assert!(out.status.success());

    let get_out = qql(&["config", "get", "url"], &home);
    assert_eq!(String::from_utf8_lossy(&get_out.stdout).trim(), test_url);
}

#[test]
fn setup_yes_alias_and_config_is_owner_only() {
    let home = temp_home("yes_perm");
    let out = qql(
        &["setup", "--url", "http://yes.example.com:6333", "--yes"],
        &home,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cfg = home.join(".qql").join("config.json");
    assert!(cfg.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config holds the API secret");
    }
}

#[test]
fn run_is_the_only_execution_subcommand() {
    let home = temp_home("aliases");
    let out = qql(&["run", "--help"], &home);
    assert!(out.status.success());
    // The old `exec` / `execute` spellings are gone: clap rejects them.
    for args in [vec!["exec", "--help"], vec!["execute", "--help"]] {
        let out = qql(&args, &home);
        assert!(
            !out.status.success(),
            "{args:?} should be rejected: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("unrecognized subcommand"),
            "{args:?} stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = qql(&["check", "--help"], &home);
    assert!(out.status.success());
}

#[test]
fn run_binds_params_with_script_file() {
    let home = temp_home("run_params");
    let dir = std::env::temp_dir().join(format!(
        "qql-run-params-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Object params bind into every statement: SHOW takes no params, so the
    // binder fails closed per statement instead of the CLI rejecting --param.
    // Fails before any backend is touched: no Qdrant needed.
    let script = dir.join("s.qql");
    std::fs::write(&script, "SHOW COLLECTIONS;\n").unwrap();
    let out = qql(&["run", script.to_str().unwrap(), "-p", "q=x"], &home);
    assert!(
        !out.status.success(),
        "a script with a failed statement must exit non-zero, even in Continue mode"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("QQL-BIND-UNSUPPORTED-STATEMENT"),
        "params must reach the binder, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Statement-scoped arrays fail closed on length mismatch, also before
    // any backend is touched.
    let two = dir.join("two.qql");
    std::fs::write(&two, "SHOW COLLECTIONS;\nSHOW COLLECTIONS;\n").unwrap();
    let params = dir.join("p.json");
    std::fs::write(&params, r#"[{"a":1}]"#).unwrap();
    let out = qql(
        &[
            "run",
            two.to_str().unwrap(),
            "--params-file",
            params.to_str().unwrap(),
        ],
        &home,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("QQL-BIND-BATCH-LENGTH"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_missing_path_fails_as_missing_script() {
    let home = temp_home("missing_path");
    let out = qql(&["run", "some_dir/missing_script"], &home);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No such file")
            || stderr.contains("no such file")
            || stderr.contains("failed to open"),
        "stderr was: {stderr}"
    );
}

#[test]
fn api_key_flag_precedence_over_config() {
    let home = temp_home("api_key_prec");
    qql(&["config", "set", "api-key", "config-secret"], &home);
    let out = qql(
        &[
            "setup",
            "--url",
            "http://localhost:6333",
            "--api-key",
            "override-secret",
            "--yes",
        ],
        &home,
    );
    assert!(out.status.success());
    let get_out = qql(&["config", "get", "api-key"], &home);
    assert_eq!(
        String::from_utf8_lossy(&get_out.stdout).trim(),
        "override-secret"
    );
}
