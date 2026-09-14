//! `qql record` — transparent Qdrant REST recorder (dev tool).
//!
//! Zero-code-change capture: point an existing app at the recorder instead of
//! Qdrant, change nothing else. Every request is forwarded to `--target`
//! byte-identically (status, headers, body), while requests with a body under
//! `/collections/` are appended as wrapped `{"method", "path", "body"}` JSONL
//! for later `qql convert` / migration use.
//!
//! Trade-offs (dev tool, not a production proxy):
//! - Bodies are buffered fully in RAM (`axum` `Bytes` → `reqwest` `Body`);
//!   multi-hundred-MB single upserts are held in memory. ColBERT-size batches
//!   are fine.
//! - Query strings are forwarded upstream and recorded as a `"query"` object
//!   (`wait`, `timeout`, `consistency`) so `qql convert` can recover `WAIT`
//!   and `PARAMS`. A trailing `/` is stripped from the recorded path;
//!   forwarding always uses the original URI.
//! - Non-JSON bodies are forwarded but not recorded (a wrapped line must hold
//!   JSON to stay convertible). Bodyless collection and quota routes
//!   (`SHOW`, `DROP COLLECTION`, `DROP INDEX`) are recorded with no `body`.
//!
//! Requires the `record` Cargo feature (opt-in so default builds stay lean).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

/// Options for [`run`]: where to listen, where to forward, what to capture.
#[derive(Debug, Clone)]
pub struct RecordOptions {
    /// Address to listen on (the app keeps pointing here).
    pub listen: SocketAddr,
    /// Upstream Qdrant base URL to forward to (e.g. `http://127.0.0.1:6334`).
    pub target: String,
    /// JSONL capture file (created/appended, fsynced per line).
    pub out: PathBuf,
    /// Optional QQL capture file (converted at record time).
    pub qql_out: Option<PathBuf>,
}

/// Run the recorder until Ctrl-C.
///
/// Binds [`RecordOptions::listen`], proxies everything to
/// [`RecordOptions::target`], and appends captures. Files are fsynced per
/// recorded line, so Ctrl-C loses nothing; a summary count is printed on exit.
pub async fn run(opts: RecordOptions) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(opts.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", opts.listen))?;
    serve(listener, opts, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

/// Serve on an already-bound listener until `shutdown` resolves.
///
/// Split from [`run`] so tests can bind ephemeral ports and shut down
/// programmatically; the Ctrl-C wiring lives in [`run`].
pub(crate) async fn serve(
    listener: tokio::net::TcpListener,
    opts: RecordOptions,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target = opts.target.trim_end_matches('/').to_string();
    if !target.starts_with("http://") && !target.starts_with("https://") {
        return Err(format!("--target must start with http:// or https://, got '{target}'").into());
    }
    if reqwest::Url::parse(&target).is_err() {
        return Err(format!("--target is not a valid URL, got '{target}'").into());
    }
    if let Some(qql_out) = opts.qql_out.as_ref()
        && same_file(&opts.out, qql_out)
    {
        return Err("--out and --qql-out must be different files".into());
    }
    let base_lines = count_lines(&opts.out).await.unwrap_or(0);
    let jsonl = open_append(&opts.out).await?;
    let qql = match &opts.qql_out {
        Some(path) => Some(open_append(path).await?),
        None => None,
    };
    // Redirects and env proxies would alter Qdrant semantics; a transparent
    // recorder must pass the upstream response through untouched.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let rec = Arc::new(Recorder {
        client,
        target,
        out_label: opts.out.display().to_string(),
        files: tokio::sync::Mutex::new(OutFiles {
            jsonl,
            qql,
            lines: base_lines,
        }),
    });
    let app = axum::Router::new().fallback(proxy).with_state(rec.clone());
    let addr = listener.local_addr()?;
    eprintln!("qql record: listening on {addr} -> {}", rec.target);
    match &opts.qql_out {
        Some(qql) => eprintln!(
            "qql record: appending JSONL to {} + QQL to {}",
            rec.out_label,
            qql.display()
        ),
        None => eprintln!("qql record: appending JSONL to {}", rec.out_label),
    }
    eprintln!("qql record: press Ctrl-C to stop (files are fsynced per line)");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    let total = rec.files.lock().await.lines - base_lines;
    eprintln!("qql record: stopped, recorded {total} request(s)");
    Ok(())
}

/// Shared proxy state: forwarding client plus the capture files.
struct Recorder {
    /// Forwarding client (redirects off, env proxies off).
    client: reqwest::Client,
    /// Upstream base URL without trailing `/`.
    target: String,
    /// `--out` path as given, used in `-- ERROR <file:line>` comments.
    out_label: String,
    /// Capture files; the mutex serializes appends so JSONL/QQL order matches.
    files: tokio::sync::Mutex<OutFiles>,
}

/// Open capture files.
struct OutFiles {
    /// Wrapped-request JSONL.
    jsonl: tokio::fs::File,
    /// Converted QQL (when `--qql-out` was given).
    qql: Option<tokio::fs::File>,
    /// JSONL lines on disk including pre-existing ones (for error comments).
    lines: u64,
}

/// Hop-by-hop headers (RFC 9110 §7.6.1, plus the common `proxy-connection`)
/// that must not be forwarded either way; `host` and `content-length` are
/// also dropped so the client/server recompute them for the new hop.
const HOP_BY_HOP: [&str; 11] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

/// Copy headers minus hop-by-hop ones, preserving multi-values.
fn forwarded(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if HOP_BY_HOP.contains(&name.as_str()) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// Path recorded into the wrapped line: query strings move to the `"query"`
/// object, and a trailing `/` is stripped (Qdrant ignores it).
fn record_path(path: &str) -> String {
    let stripped = path.strip_suffix('/').unwrap_or(path);
    if stripped.is_empty() {
        "/".to_string()
    } else {
        stripped.to_string()
    }
}

/// Record collection and quota routes. Bodyless GETs/DELETEs are included;
/// non-JSON bodies are skipped.
fn in_record_scope(path: &str) -> bool {
    let trimmed = path.trim_start_matches('/');
    trimmed == "collections"
        || trimmed.starts_with("collections/")
        || trimmed == "quotas"
        || trimmed.starts_with("quotas/")
}

fn should_record(path: &str, body: &[u8]) -> bool {
    if !in_record_scope(path) {
        return false;
    }
    body.is_empty() || serde_json::from_slice::<serde_json::Value>(body).is_ok()
}

/// Forward any request to `--target` and capture convertible ones.
///
/// Recording failures are logged and never break forwarding.
async fn proxy(State(rec): State<Arc<Recorder>>, req: Request) -> Response {
    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/".to_string());
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let headers = forwarded(req.headers());
    // Buffer fully: simple and byte-exact; huge single upserts sit in RAM
    // (documented dev-tool trade-off).
    let body = match to_bytes(req.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(e) => {
            return error_response(StatusCode::BAD_GATEWAY, &format!("cannot read body: {e}"));
        }
    };
    let url = format!("{}{}", rec.target, path_and_query);
    let upstream_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(upstream_method) => upstream_method,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "unsupported method"),
    };
    let upstream = match rec
        .client
        .request(upstream_method, url)
        .headers(headers)
        .body(body.clone())
        .send()
        .await
    {
        Ok(upstream) => upstream,
        Err(e) => {
            eprintln!("{method} {path} -> upstream error: {e}");
            return error_response(StatusCode::BAD_GATEWAY, &format!("upstream error: {e}"));
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = forwarded(upstream.headers());
    let resp_body = match upstream.bytes().await {
        Ok(resp_body) => resp_body,
        Err(e) => return error_response(StatusCode::BAD_GATEWAY, &format!("upstream error: {e}")),
    };
    let recorded_path = record_path(&path);
    if should_record(&recorded_path, &body) {
        let json = if body.is_empty() {
            None
        } else {
            Some(serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({})))
        };
        record_request(&rec, method.as_str(), &recorded_path, &query, json.as_ref()).await;
        eprintln!("{method} {recorded_path} -> {}", status.as_u16());
    } else if !body.is_empty() && in_record_scope(&recorded_path) {
        eprintln!(
            "{method} {recorded_path} -> {} (not recorded: body is not JSON)",
            status.as_u16()
        );
    }
    let mut out = Response::new(Body::from(resp_body));
    *out.status_mut() = status;
    for (name, value) in resp_headers {
        if let Some(name) = name {
            out.headers_mut().append(name, value);
        }
    }
    out
}

/// Append one wrapped line (plus its QQL conversion) to the capture files.
///
/// File errors are logged, never propagated: capture must not break
/// forwarding.
async fn record_request(
    rec: &Arc<Recorder>,
    method: &str,
    path: &str,
    query: &str,
    body: Option<&serde_json::Value>,
) {
    let mut line = serde_json::json!({"method": method, "path": path});
    if !query.is_empty() {
        line["query"] = query_object(query);
    }
    if let Some(body) = body {
        line["body"] = body.clone();
    }
    let line = line.to_string();
    let mut files = rec.files.lock().await;
    if let Err(e) = write_line(&mut files.jsonl, line.as_str()).await {
        eprintln!("qql record: cannot write {}: {e}", rec.out_label);
        return;
    }
    files.lines += 1;
    let lineno = files.lines;
    let Some(qql) = files.qql.as_mut() else {
        return;
    };
    match qql_convert::convert(&line, None) {
        Ok(stmts) => {
            for stmt in &stmts {
                if let Err(e) = write_line(qql, &format!("{stmt};")).await {
                    eprintln!("qql record: cannot write QQL capture: {e}");
                    return;
                }
            }
        }
        Err(e) => {
            if let Err(e) =
                write_line(qql, &format!("-- ERROR {}:{lineno} {e}", rec.out_label)).await
            {
                eprintln!("qql record: cannot write QQL capture: {e}");
            }
        }
    }
}

/// Split `a=b&c=d` into a JSON object of string values.
fn query_object(raw: &str) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        obj.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    serde_json::Value::Object(obj)
}

/// Append one `\n`-terminated line and fsync (dev-tool durability).
async fn write_line(
    file: &mut tokio::fs::File,
    line: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncWriteExt;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

/// Count existing lines so `-- ERROR <file:line>` numbers stay cumulative
/// across appends. Missing/unreadable files start at zero. Streams in 8 KiB
/// chunks so large captures never sit fully in RAM, and stays async so the
/// runtime is not blocked at startup.
async fn count_lines(path: &std::path::Path) -> Result<u64, std::io::Error> {
    use tokio::io::AsyncReadExt;
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut buf = [0u8; 8192];
    let mut lines = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        lines += buf[..n].iter().filter(|b| **b == b'\n').count() as u64;
    }
    Ok(lines)
}

/// Two paths name the same file when their canonical forms match, or when
/// neither exists yet and their absolute forms match (`./x` vs `x`,
/// symlinked parents). Falls back to `false` when neither comparison applies.
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return ca == cb;
    }
    absolutize(a) == absolutize(b)
}

/// Best-effort absolute path without touching the filesystem.
fn absolutize(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let mut cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    cwd.push(path);
    cwd
}

/// Open (creating parents as needed) for appending.
async fn open_append(
    path: &std::path::Path,
) -> Result<tokio::fs::File, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?)
}

/// Plain-text error response for forwarding failures (upstream unreachable,
/// bad method). Normal upstream errors pass through untouched above.
fn error_response(status: StatusCode, message: &str) -> Response {
    let mut out = Response::new(Body::from(message.to_string()));
    *out.status_mut() = status;
    out
}

#[cfg(test)]
mod tests {
    use super::{forwarded, record_path, same_file, should_record};

    #[test]
    fn forwarded_preserves_repeated_headers() {
        let mut headers = axum::http::HeaderMap::new();
        headers.append("x-multi", "a".parse().unwrap());
        headers.append("x-multi", "b".parse().unwrap());
        let out = forwarded(&headers);
        let values: Vec<_> = out.get_all("x-multi").iter().collect();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn same_file_matches_dot_qualified_paths() {
        assert!(same_file(
            std::path::Path::new("capture.jsonl"),
            std::path::Path::new("./capture.jsonl")
        ));
        assert!(!same_file(
            std::path::Path::new("capture.jsonl"),
            std::path::Path::new("other.jsonl")
        ));
    }

    #[test]
    fn record_path_strips_query_context_and_trailing_slash() {
        assert_eq!(
            record_path("/collections/docs/points/query"),
            "/collections/docs/points/query"
        );
        assert_eq!(record_path("/collections/docs/"), "/collections/docs");
        assert_eq!(record_path("/"), "/");
    }

    #[test]
    fn should_record_covers_collection_and_quota_routes() {
        let body = br#"{"vector": [0.1], "limit": 1}"#;
        assert!(should_record("/collections/docs/points/query", body));
        assert!(should_record("/collections/docs/points", body));
        // Bodyless SHOW / DROP: recorded with no `body` field.
        assert!(should_record("/collections/docs", b""));
        assert!(should_record("/collections", b""));
        assert!(should_record("/quotas", b""));
        // Outside collection/quota routes: forwarded, never recorded.
        assert!(!should_record("/cluster", body));
        assert!(!should_record("/healthz", b""));
        // Non-JSON bodies cannot form a convertible wrapped line.
        assert!(!should_record("/collections/docs/points", b"\x00\x01"));
    }
}
