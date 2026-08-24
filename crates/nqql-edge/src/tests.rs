use super::*;
use qql_core::parser::Parser;
#[cfg(feature = "http-embedding")]
use std::io::{Read, Write};
#[cfg(feature = "http-embedding")]
use std::net::TcpListener;
#[cfg(feature = "http-embedding")]
use std::sync::mpsc;
#[cfg(feature = "http-embedding")]
use std::time::Duration;

#[test]
fn local_executor_options_default_has_no_sparse_model() {
    let opts = LocalExecutorOptions::default();
    assert!(opts.sparse_model.is_none());
}

#[test]
fn local_executor_options_with_sparse_model() {
    let opts = LocalExecutorOptions {
        sparse_model: Some("splade".into()),
        ..Default::default()
    };
    assert_eq!(opts.sparse_model.as_deref(), Some("splade"));
}

#[test]
fn standalone_local_opts_camel_case_sparse_model() {
    let opts = serde_json::json!({ "sparseModel": "bge-m3" });
    let lo = standalone_local_opts(Some(&opts));
    assert_eq!(lo.sparse_model.as_deref(), Some("bge-m3"));
}

#[test]
fn standalone_local_opts_snake_case_sparse_model() {
    let opts = serde_json::json!({ "sparse_model": "splade" });
    let lo = standalone_local_opts(Some(&opts));
    assert_eq!(lo.sparse_model.as_deref(), Some("splade"));
}

#[test]
fn standalone_local_opts_no_sparse_model_is_none() {
    let opts = serde_json::json!({});
    let lo = standalone_local_opts(Some(&opts));
    assert!(lo.sparse_model.is_none());
}

// ═══════════════════════════════════════════════════════════════
//  Default-feature HTTP embedding coverage (Finding 1 + Finding 3
//  of docs/audits/review-adapters-website.json): prove the native
//  `httpExecutor` binding exists and `executeStmt` selects HTTP
//  embedding whenever `embedUrl` is supplied — against a local mock
//  OpenAI-compatible endpoint, so no network is involved.
// ═══════════════════════════════════════════════════════════════

/// A request captured by the mock embedding server.
#[cfg(feature = "http-embedding")]
#[derive(Debug)]
struct MockEmbedRequest {
    method: String,
    path: String,
    auth: Option<String>,
    body: String,
}

#[cfg(feature = "http-embedding")]
impl MockEmbedRequest {
    fn model(&self) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()?
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    fn inputs(&self) -> Vec<String> {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|v| v.get("input").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// Minimal OpenAI-compatible `/v1/embeddings` mock. Accepts one connection,
/// parses the HTTP request, replies with a fixed dense vector, and sends the
/// captured request to `tx`. Returns the base URL to point `embedUrl` at.
#[cfg(feature = "http-embedding")]
fn spawn_mock_embedding_server(dim: usize, tx: mpsc::Sender<MockEmbedRequest>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock embedding server");
    let addr = listener.local_addr().expect("mock server address");
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("mock embedding server connection");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        // Read headers (up to \r\n\r\n) plus the declared Content-Length body.
        let (header_end, body_len) = loop {
            let n = stream.read(&mut chunk).expect("read request headers");
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buf[..end]);
                let len = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        if key.eq_ignore_ascii_case("content-length") {
                            value.trim().parse().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                break (end, len);
            }
        };
        while buf.len() < header_end + 4 + body_len {
            let n = stream.read(&mut chunk).expect("read request body");
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
        }

        let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
        let body =
            String::from_utf8_lossy(&buf[header_end + 4..header_end + 4 + body_len]).into_owned();
        let mut parts = header_text
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let auth = header_text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.eq_ignore_ascii_case("authorization") {
                Some(value.trim().to_string())
            } else {
                None
            }
        });
        let _ = tx.send(MockEmbedRequest {
            method,
            path,
            auth,
            body,
        });

        let embedding: Vec<f32> = (0..dim).map(|i| (i + 1) as f32 / 10.0).collect();
        let payload = serde_json::json!({
            "data": [{ "index": 0, "embedding": embedding }]
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    });
    format!("http://{addr}/v1/embeddings")
}

#[test]
#[cfg(feature = "http-embedding")]
fn http_executor_native_symbol_constructs_client() {
    let data_dir = std::env::temp_dir().join(format!("nqql-edge-http-ctor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let client = http_executor(
        data_dir.to_string_lossy().into_owned(),
        "http://127.0.0.1:1/v1/embeddings".to_string(),
        "key".to_string(),
        "mock".to_string(),
        4,
        Some(false),
    )
    .expect("http_executor must construct a client");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let report = client
            .inner
            .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
            .await
            .expect("SHOW COLLECTIONS via httpExecutor");
        assert!(report.ok, "SHOW COLLECTIONS failed: {report:?}");
        client
            .inner
            .close()
            .await
            .expect("close httpExecutor client");
    });
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
#[cfg(feature = "http-embedding")]
fn execute_stmt_prefers_http_embedding_when_embed_url_supplied() {
    let (tx, rx) = mpsc::channel();
    let url = spawn_mock_embedding_server(4, tx);
    let data_dir = std::env::temp_dir().join(format!("nqql-edge-http-stmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);

    let stmt = Stmt {
        inner: Parser::parse("UPSERT INTO http_docs VALUES {id: 1, text: 'hello'}")
            .expect("parse upsert"),
    };
    let options = serde_json::json!({
        "dataDir": data_dir,
        "onDiskPayload": false,
        "embedUrl": url,
        "embedKey": "test-key",
        "embedModel": "mock-embed",
        "embedDim": 4,
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let report = runtime.block_on(async {
        let raw = execute_stmt(&stmt, Some(options))
            .await
            .expect("execute_stmt with embedUrl");
        serde_json::from_str::<serde_json::Value>(&raw).expect("report is JSON")
    });

    assert!(
        report["ok"].as_bool().unwrap_or(false),
        "execute_stmt report: {report}"
    );
    let req = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("mock embedding server received a request");
    assert_eq!(req.method, "POST", "request: {req:?}");
    assert_eq!(req.path, "/v1/embeddings", "request: {req:?}");
    assert_eq!(
        req.model().as_deref(),
        Some("mock-embed"),
        "request: {req:?}"
    );
    assert_eq!(req.inputs(), vec!["hello".to_string()], "request: {req:?}");
    assert_eq!(
        req.auth.as_deref(),
        Some("Bearer test-key"),
        "request: {req:?}"
    );
    assert!(
        rx.try_recv().is_err(),
        "expected exactly one embedding request, got more"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
#[cfg(not(feature = "http-embedding"))]
fn execute_stmt_rejects_embed_url_when_http_embedding_disabled() {
    let stmt = Stmt {
        inner: Parser::parse("COUNT FROM docs").expect("parse count"),
    };
    let options = serde_json::json!({
        "dataDir": std::env::temp_dir().join(format!(
            "nqql-edge-http-reject-{}",
            std::process::id()
        )),
        "embedUrl": "http://127.0.0.1:1/v1/embeddings",
        "embedKey": "test-key",
        "embedModel": "mock-embed",
        "embedDim": 4,
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let err = runtime.block_on(async {
        execute_stmt(&stmt, Some(options))
            .await
            .expect_err("embedUrl must be rejected without http-embedding")
    });
    assert!(
        err.to_string().contains("http-embedding"),
        "unexpected error: {err}"
    );
}
