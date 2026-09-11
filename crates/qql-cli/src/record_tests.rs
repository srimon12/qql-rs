//! Integration tests for `qql record` (record builds only).
//!
//! An in-process mock upstream (axum) plays Qdrant; the recorder from
//! [`crate::record`] proxies to it. Covers: byte-identical responses
//! (status/headers/body), wrapped-JSONL validity, `--qql-out` conversion that
//! parses, search + upsert bodies, GET-without-body passthrough, query-string
//! preservation, and `# ERROR` comments for unconvertible endpoints.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::record::{RecordOptions, serve};

/// One request observed by the mock upstream.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path_query: String,
    api_key: Option<String>,
    body: Vec<u8>,
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "qql-record-it-{}-{nanos}-{tag}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn wait_ready(addr: SocketAddr) {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("server on {addr} never became ready");
}

/// Mock Qdrant: records what it saw, answers a fixed JSON body plus a custom
/// header so tests can assert byte-identical proxying.
async fn start_mock(seen: Arc<Mutex<Vec<Seen>>>) -> SocketAddr {
    async fn echo(
        axum::extract::State(seen): axum::extract::State<Arc<Mutex<Vec<Seen>>>>,
        req: axum::extract::Request,
    ) -> axum::response::Response {
        let method = req.method().to_string();
        let path_query = req
            .uri()
            .path_and_query()
            .map(|pq| pq.to_string())
            .unwrap_or_default();
        let api_key = req
            .headers()
            .get("api-key")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        seen.lock().unwrap().push(Seen {
            method,
            path_query,
            api_key,
            body,
        });
        axum::response::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("x-mock", "mock-1")
            .body(axum::body::Body::from(r#"{"status":"ok"}"#))
            .unwrap()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().fallback(echo).with_state(seen),
        )
        .await
        .expect("mock serve");
    });
    wait_ready(addr).await;
    addr
}

/// Recorder under test on an ephemeral port, capturing into `dir`.
struct RecorderUnderTest {
    addr: SocketAddr,
    out: PathBuf,
    qql_out: PathBuf,
    _handle: tokio::task::JoinHandle<()>,
}

async fn start_recorder(target: SocketAddr, dir: &Path) -> RecorderUnderTest {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind recorder");
    let addr = listener.local_addr().expect("recorder addr");
    let out = dir.join("capture.jsonl");
    let qql_out = dir.join("capture.qql");
    let opts = RecordOptions {
        listen: addr,
        target: format!("http://{target}"),
        out: out.clone(),
        qql_out: Some(qql_out.clone()),
    };
    let handle = tokio::spawn(async move {
        let _ = serve(listener, opts, std::future::pending()).await;
    });
    wait_ready(addr).await;
    RecorderUnderTest {
        addr,
        out,
        qql_out,
        _handle: handle,
    }
}

fn parse_jsonl(path: &Path) -> Vec<serde_json::Value> {
    let data = std::fs::read_to_string(path).expect("read jsonl");
    data.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("jsonl line is valid JSON"))
        .collect()
}

#[tokio::test]
async fn proxies_byte_identically_and_captures_search_upsert() {
    let dir = temp_dir("main");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mock = start_mock(seen.clone()).await;
    let rec = start_recorder(mock, &dir).await;
    let client = reqwest::Client::new();
    let base = format!("http://{}", rec.addr);

    // Search body: must be recorded and convertible.
    let search = serde_json::json!({"query": {"nearest": [0.1, 0.2]}, "limit": 5});
    let resp = client
        .post(format!("{base}/collections/docs/points/query"))
        .json(&search)
        .send()
        .await
        .expect("search through recorder");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get("x-mock").and_then(|v| v.to_str().ok()),
        Some("mock-1")
    );
    assert_eq!(
        resp.bytes().await.expect("search body").as_ref(),
        br#"{"status":"ok"}"#
    );

    // Upsert body with a query string and auth header: query + auth must reach
    // upstream byte-identically; the recorded path drops the query.
    let upsert = serde_json::json!({"points": [{"id": 1, "vector": [0.1, 0.2], "payload": {"title": "hello"}}]});
    let resp = client
        .put(format!("{base}/collections/docs/points?wait=true"))
        .header("api-key", "secret")
        .json(&upsert)
        .send()
        .await
        .expect("upsert through recorder");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.bytes().await.expect("upsert body").as_ref(),
        br#"{"status":"ok"}"#
    );

    // GET with no body: forwarded, never recorded.
    let resp = client
        .get(format!("{base}/collections/docs"))
        .send()
        .await
        .expect("get through recorder");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get("x-mock").and_then(|v| v.to_str().ok()),
        Some("mock-1")
    );
    assert_eq!(
        resp.bytes().await.expect("get body").as_ref(),
        br#"{"status":"ok"}"#
    );

    // Upstream saw everything exactly as sent.
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path_query, "/collections/docs/points/query");
    assert_eq!(seen[0].body, serde_json::to_vec(&search).expect("json"));
    assert_eq!(seen[1].method, "PUT");
    assert_eq!(seen[1].path_query, "/collections/docs/points?wait=true");
    assert_eq!(seen[1].api_key.as_deref(), Some("secret"));
    assert_eq!(seen[1].body, serde_json::to_vec(&upsert).expect("json"));
    assert_eq!(seen[2].method, "GET");
    assert_eq!(seen[2].path_query, "/collections/docs");

    // JSONL holds exactly the two bodied requests as wrapped lines.
    let lines = parse_jsonl(&rec.out);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["method"], "POST");
    assert_eq!(lines[0]["path"], "/collections/docs/points/query");
    assert_eq!(lines[0]["body"], search);
    assert_eq!(lines[1]["method"], "PUT");
    assert_eq!(lines[1]["path"], "/collections/docs/points");
    assert_eq!(lines[1]["body"], upsert);

    // --qql-out converted at record time. `# ERROR` lines are capture
    // annotations, so the rest must parse as one whole script (not
    // line-by-line: emitted statements may span lines).
    let qql = std::fs::read_to_string(&rec.qql_out).expect("read qql-out");
    let stmts = script_statements(&qql);
    assert_eq!(stmts.len(), 2, "{stmts:?}");
    assert_eq!(stmts[0], "QUERY [0.1, 0.2] FROM docs LIMIT 5");
    assert!(stmts[1].starts_with("UPSERT INTO docs"), "{}", stmts[1]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Parse a `--qql-out` capture as a script, skipping `# ERROR` annotations.
fn script_statements(capture: &str) -> Vec<String> {
    let script: String = capture
        .lines()
        .filter(|line| !line.starts_with("# ERROR "))
        .collect::<Vec<_>>()
        .join("\n");
    crate::script::split_statements(&script).expect("qql-out parses as a script")
}

#[tokio::test]
async fn qql_out_multiline_statement_parses_as_script() {
    let dir = temp_dir("multiline");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mock = start_mock(seen.clone()).await;
    let rec = start_recorder(mock, &dir).await;
    let client = reqwest::Client::new();

    // A batch upsert formats as a multi-line statement; it must still read
    // back as one statement of the capture script.
    let batch = serde_json::json!({
        "batch": {
            "ids": [1, 2],
            "vectors": [[0.1], [0.2]],
            "payloads": [{"a": 1}, {"a": 2}],
        }
    });
    let resp = client
        .put(format!("http://{}/collections/docs/points", rec.addr))
        .json(&batch)
        .send()
        .await
        .expect("batch through recorder");
    assert_eq!(resp.status().as_u16(), 200);

    let qql = std::fs::read_to_string(&rec.qql_out).expect("read qql-out");
    let stmts = script_statements(&qql);
    assert_eq!(stmts.len(), 1, "{stmts:?}");
    assert!(stmts[0].contains('\n'), "expected a multi-line statement");
    let reparsed = qql_core::parser::Parser::parse(&format!("{};", stmts[0]))
        .unwrap_or_else(|e| panic!("multi-line statement failed to parse: {e}"));
    assert_eq!(
        qql_core::fmt::format_stmt(&reparsed),
        stmts[0],
        "multi-line capture statement is not canonical"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unconvertible_endpoints_record_jsonl_and_error_comment() {
    let dir = temp_dir("error");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mock = start_mock(seen.clone()).await;
    let rec = start_recorder(mock, &dir).await;
    let client = reqwest::Client::new();

    // No QQL mapping for the aliases helper: forwarding still works.
    let resp = client
        .post(format!("http://{}/collections/docs/aliases", rec.addr))
        .json(&serde_json::json!({"actions": []}))
        .send()
        .await
        .expect("aliases through recorder");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.bytes().await.expect("aliases body").as_ref(),
        br#"{"status":"ok"}"#
    );

    // JSONL still captures the wrapped request ...
    let lines = parse_jsonl(&rec.out);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["method"], "POST");
    assert_eq!(lines[0]["path"], "/collections/docs/aliases");

    // ... while --qql-out holds a `# ERROR <file:line> <error>` comment that
    // never broke forwarding.
    let qql = std::fs::read_to_string(&rec.qql_out).expect("read qql-out");
    let errors: Vec<&str> = qql
        .lines()
        .filter(|line| line.starts_with("# ERROR "))
        .collect();
    assert_eq!(errors.len(), 1, "{qql:?}");
    assert!(errors[0].contains(":1 "), "line number: {}", errors[0]);
    assert!(errors[0].contains("unsupported endpoint"), "{}", errors[0]);

    let _ = std::fs::remove_dir_all(&dir);
}
