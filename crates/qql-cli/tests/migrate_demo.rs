//! Guards for the berlin sharded-migration demo script.
//!
//! The script lives in `examples/migrate/` (outside the crate source tree).
//! Its syntax is checked on every `cargo test`; the full multi-node flow is
//! behind `#[ignore]` because it needs a live cluster and the `vs-qdrant`
//! corpus.

use std::path::PathBuf;
use std::process::Command;

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/migrate/berlin_shard_migration.py")
}

/// Every CLI invocation inside the demo must at least be valid Python.
#[test]
fn demo_script_parses() {
    let path = script_path();
    let source = std::fs::read_to_string(&path).expect("demo script exists");
    let Ok(output) = Command::new("python3")
        .arg("-c")
        .arg("import ast, sys; ast.parse(sys.stdin.read())")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("piped stdin")
                .write_all(source.as_bytes())?;
            child.wait_with_output()
        })
    else {
        // No python3 on this host: the syntax guard is best-effort.
        return;
    };
    assert!(
        output.status.success(),
        "demo script does not parse: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Full end-to-end run: 3-node Qdrant on :6333/:7333/:8333 plus
/// `vs-qdrant/data/` corpus. Build the CLI first:
///
/// ```sh
/// cargo build -p qql-cli
/// cargo test -p qql-cli --test migrate_demo -- --ignored
/// ```
#[test]
#[ignore = "requires a live multi-node Qdrant cluster and the vs-qdrant corpus"]
fn demo_script_end_to_end() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let binary = root.join("target/debug/qql");
    assert!(
        binary.exists(),
        "build the CLI first: cargo build -p qql-cli ({})",
        binary.display()
    );
    let status = Command::new("python3")
        .arg(script_path())
        .arg("--qql")
        .arg(&binary)
        .arg("--clean")
        .status()
        .expect("run the berlin migration demo");
    assert!(status.success(), "berlin migration demo failed");
}
