//! End-to-end `qql fmt` regression tests at the process boundary.
//!
//! Pins the exit codes, the single trailing newline on stdout, and the
//! `--write` normalization that `qql fmt --check` relies on.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const UNFORMATTED: &str = "query [0.1, 0.2] from docs limit 5;\n";
const FORMATTED: &str = "QUERY [0.1, 0.2] FROM docs LIMIT 5;\n";

fn temp_file(tag: &str, contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("qql-fmt-it-{}-{nanos}", std::process::id()));
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
fn check_passes_on_canonical_input() {
    let path = temp_file("canonical", FORMATTED);
    let out = qql(&["fmt", "--check", path.to_str().expect("utf8 path")]);
    assert!(
        out.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_fails_on_unformatted_input() {
    let path = temp_file("unformatted", UNFORMATTED);
    let out = qql(&["fmt", "--check", path.to_str().expect("utf8 path")]);
    assert!(!out.status.success(), "check unexpectedly passed");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is not formatted"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn stdout_is_canonical_with_exactly_one_trailing_newline() {
    let path = temp_file("stdout", UNFORMATTED);
    let out = qql(&["fmt", path.to_str().expect("utf8 path")]);
    assert!(out.status.success());
    // Byte equality catches both a missing and a doubled trailing newline.
    assert_eq!(String::from_utf8_lossy(&out.stdout), FORMATTED);
}

#[test]
fn write_normalizes_and_recheck_passes() {
    let path = temp_file("write", UNFORMATTED);
    let path = path.to_str().expect("utf8 path");
    let out = qql(&["fmt", "--write", path]);
    assert!(
        out.status.success(),
        "write failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(path).expect("read"), FORMATTED);
    assert!(qql(&["fmt", "--check", path]).status.success());
}

#[test]
fn stdin_prints_canonical_output() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qql"))
        .args(["fmt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qql");
    {
        use std::io::Write as _;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(UNFORMATTED.as_bytes())
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), FORMATTED);
}
