//! W2 scroll/ordering/lookup decode: REST JSON → QQL → replan route equality.

use qql_convert::convert;
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;
use qql_plan::{plan, to_rest_route};

/// Plan → route → convert → reparse → replan must yield the same route.
fn assert_parity(source: &str) {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    let body = route.body_json();
    let wrapped = serde_json::json!({
        "method": route.method.as_str(),
        "path": route.path,
        "body": body,
    })
    .to_string();

    let emitted =
        convert(&wrapped, None).unwrap_or_else(|e| panic!("convert {source} ({wrapped}): {e}"));
    assert_eq!(emitted.len(), 1, "{source} must convert to one statement");
    let reparsed = Parser::parse(&format!("{};", emitted[0]))
        .unwrap_or_else(|e| panic!("reparse {}: {e}", emitted[0]));
    assert_eq!(
        format_stmt(&reparsed),
        emitted[0],
        "converted {source} is not canonical"
    );
    let replanned = plan(&reparsed).unwrap_or_else(|e| panic!("replan {}: {e}", emitted[0]));
    let reroute =
        to_rest_route(&replanned).unwrap_or_else(|e| panic!("reroute {}: {e:?}", emitted[0]));
    assert_eq!(reroute.method.as_str(), route.method.as_str(), "{source}");
    assert_eq!(reroute.path, route.path, "{source}");
    assert_eq!(
        reroute.body_json(),
        body,
        "{source}\nemitted: {}",
        emitted[0]
    );
}

fn convert_scroll(body: serde_json::Value) -> String {
    let wrapped = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/scroll",
        "body": body,
    })
    .to_string();
    let out = convert(&wrapped, None).expect("convert scroll");
    assert_eq!(out.len(), 1);
    out.into_iter().next().unwrap()
}

#[test]
fn scroll_order_by_decodes() {
    assert_eq!(
        convert_scroll(serde_json::json!({"order_by": "created_at", "limit": 10})),
        "SCROLL FROM docs ORDER BY created_at ASC LIMIT 10"
    );
    assert_eq!(
        convert_scroll(
            serde_json::json!({"order_by": {"key": "created_at", "direction": "desc"}, "limit": 10})
        ),
        "SCROLL FROM docs ORDER BY created_at DESC LIMIT 10"
    );
    assert_eq!(
        convert_scroll(serde_json::json!({
            "order_by": {"key": "score", "direction": "desc", "start_from": 100},
            "limit": 10
        })),
        "SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10"
    );
    // Unknown order_by members and bad start values fail closed.
    for bad in [
        serde_json::json!({"order_by": {"key": "k", "bogus": 1}}),
        serde_json::json!({"order_by": {"key": "k", "start_from": true}}),
        serde_json::json!({"order_by": {"key": "k", "start_from": [1]}}),
        serde_json::json!({"order_by": {"direction": "up", "key": "k"}}),
    ] {
        let wrapped = serde_json::json!({
            "method": "POST",
            "path": "/collections/docs/points/scroll",
            "body": bad,
        })
        .to_string();
        assert!(convert(&wrapped, None).is_err(), "must fail: {bad}");
    }
}

#[test]
fn scroll_payload_decodes() {
    assert_eq!(
        convert_scroll(serde_json::json!({"with_payload": false, "limit": 10})),
        "SCROLL FROM docs WITH PAYLOAD false LIMIT 10"
    );
    assert_eq!(
        convert_scroll(serde_json::json!({"with_payload": {"include": ["title"]}, "limit": 10})),
        "SCROLL FROM docs WITH PAYLOAD INCLUDE (title) LIMIT 10"
    );
    assert_eq!(
        convert_scroll(serde_json::json!({"with_payload": {"exclude": ["secret"]}, "limit": 10})),
        "SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10"
    );
    // Wire `true` is the omitted default.
    assert_eq!(
        convert_scroll(serde_json::json!({"with_payload": true, "limit": 10})),
        "SCROLL FROM docs LIMIT 10"
    );
}

#[test]
fn query_order_by_start_from_decodes() {
    let wrapped = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/query",
        "body": {"query": {"order_by": {"key": "score", "direction": "desc", "start_from": 4.5}}, "limit": 5},
    })
    .to_string();
    let out = convert(&wrapped, None).expect("convert order_by");
    assert_eq!(
        out[0],
        "QUERY ORDER BY score DESC START FROM 4.5 FROM docs LIMIT 5"
    );
}

#[test]
fn group_lookup_full_decodes() {
    let wrapped = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/query/groups",
        "body": {
            "query": {"order_by": "rank"},
            "group_by": "topic",
            "group_size": 3,
            "limit": 10,
            "with_lookup": {
                "collection": "topics",
                "with_payload": {"include": ["title"]},
                "with_vectors": ["dense"],
            },
        },
    })
    .to_string();
    let out = convert(&wrapped, None).expect("convert groups");
    assert!(
        out[0].contains("LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense)"),
        "got: {}",
        out[0]
    );
}

#[test]
fn lookup_from_shard_key_decodes() {
    // Shard-carrying lookup_from must match a PREFETCH LOOKUP clause to decode.
    let wrapped = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/query",
        "body": {
            "query": {"fusion": "rrf"},
            "prefetch": [{
                "query": {"nearest": {"text": "x", "model": "e5"}},
                "using": "dense",
                "limit": 50,
                "lookup_from": {"collection": "docs2", "vector": "dense", "shard_key": "acme"},
            }],
            "limit": 10,
            "lookup_from": {"collection": "docs2", "vector": "dense", "shard_key": "acme"},
        },
    })
    .to_string();
    let out = convert(&wrapped, None).expect("convert lookup shard");
    assert!(out[0].contains("SHARD 'acme'"), "got: {}", out[0]);
    // Multi-key shard selectors have no single-key QQL form.
    let bad = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/query",
        "body": {
            "query": {"fusion": "rrf"},
            "prefetch": [{
                "query": {"nearest": {"text": "x", "model": "e5"}},
                "limit": 50,
                "lookup_from": {"collection": "c", "shard_key": ["a", "b"]},
            }],
            "limit": 10,
            "lookup_from": {"collection": "c", "shard_key": ["a", "b"]},
        },
    })
    .to_string();
    assert!(convert(&bad, None).is_err());
}

#[test]
fn w2_shapes_route_parity() {
    for source in [
        "SCROLL FROM docs ORDER BY created_at DESC LIMIT 10;",
        "SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD false LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD INCLUDE (title, url) LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10;",
        "SCROLL FROM docs WHERE status = 'active' AFTER 7 ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' SHARD 'acme' WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 10;",
        "QUERY ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' FROM docs LIMIT 20;",
        "QUERY ORDER BY score ASC START FROM 100 FROM docs LIMIT 5;",
        "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 20;",
        "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) QUERY FUSION RRF FROM docs PREFETCH (a LOOKUP FROM docs2 VECTOR dense SHARD 'acme') LIMIT 10;",
    ] {
        assert_parity(source);
    }
}
