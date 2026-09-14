//! W2 scroll/ordering/lookup lowering: AST → plan → REST body shapes.

use qql_core::parser::Parser;
use qql_plan::PlannedOperation;
use qql_plan::{plan, to_rest_route};

fn body(source: &str) -> serde_json::Value {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    route.body_json().expect("body")
}

#[test]
fn scroll_order_by_lowers_to_wire() {
    let json = body("SCROLL FROM docs ORDER BY created_at DESC LIMIT 10;");
    assert_eq!(json["order_by"]["key"], "created_at");
    assert_eq!(json["order_by"]["direction"], "desc");
    // Plain scrolls omit the key entirely.
    let plain = body("SCROLL FROM docs LIMIT 10;");
    assert!(plain.get("order_by").is_none());
}

#[test]
fn scroll_order_by_start_from_lowers() {
    let json = body("SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10;");
    assert_eq!(json["order_by"]["start_from"], 100);
    let dt = body(
        "SCROLL FROM docs ORDER BY created_at ASC START FROM '2024-01-01T00:00:00Z' LIMIT 10;",
    );
    assert_eq!(dt["order_by"]["start_from"], "2024-01-01T00:00:00Z");
}

#[test]
fn scroll_payload_selectors_lower() {
    // Default stays `true`.
    assert_eq!(body("SCROLL FROM docs LIMIT 10;")["with_payload"], true);
    assert_eq!(
        body("SCROLL FROM docs WITH PAYLOAD false LIMIT 10;")["with_payload"],
        false
    );
    assert_eq!(
        body("SCROLL FROM docs WITH PAYLOAD INCLUDE (title, url) LIMIT 10;")["with_payload"],
        serde_json::json!({"include": ["title", "url"]})
    );
    assert_eq!(
        body("SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10;")["with_payload"],
        serde_json::json!({"exclude": ["secret"]})
    );
}

#[test]
fn query_order_by_start_from_lowers() {
    let json = body(
        "QUERY ORDER BY created_at DESC START FROM '2024-06-01T00:00:00Z' FROM docs LIMIT 10;",
    );
    assert_eq!(json["query"]["order_by"]["key"], "created_at");
    assert_eq!(json["query"]["order_by"]["direction"], "desc");
    assert_eq!(
        json["query"]["order_by"]["start_from"],
        "2024-06-01T00:00:00Z"
    );
    // Without START FROM the key is absent.
    let plain = body("QUERY ORDER BY rank ASC FROM docs LIMIT 5;");
    assert!(plain["query"]["order_by"].get("start_from").is_none());
}

#[test]
fn group_lookup_full_selector_lowers() {
    let json = body(
        "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 20;",
    );
    assert_eq!(json["with_lookup"]["collection"], "topics");
    assert_eq!(
        json["with_lookup"]["with_payload"],
        serde_json::json!({"include": ["title"]})
    );
    assert_eq!(
        json["with_lookup"]["with_vectors"],
        serde_json::json!(["dense"])
    );
    // Bare lookup stays a bare collection name.
    let bare =
        body("QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic LOOKUP FROM topics LIMIT 20;");
    assert_eq!(bare["with_lookup"], "topics");
}

#[test]
fn prefetch_lookup_shard_key_lowers() {
    let json = body(
        "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) QUERY FUSION RRF FROM docs PREFETCH (a LOOKUP FROM docs2 VECTOR dense SHARD 'acme') LIMIT 10;",
    );
    assert_eq!(json["lookup_from"]["collection"], "docs2");
    assert_eq!(json["lookup_from"]["vector"], "dense");
    assert_eq!(json["lookup_from"]["shard_key"], "acme");
    // The prefetch stage carries the same lookup.
    assert_eq!(json["prefetch"][0]["lookup_from"]["shard_key"], "acme");
    // Numeric shard keys stay numeric.
    let num = body(
        "QUERY TEXT 'x' FROM docs USING dense PREFETCH (QUERY TEXT 'y' FROM docs USING dense LIMIT 5 LOOKUP FROM c SHARD 101) LIMIT 10;",
    );
    assert_eq!(num["lookup_from"]["shard_key"], 101);
}

#[test]
fn scroll_plans_to_scroll_operation() {
    let stmt =
        Parser::parse("SCROLL FROM docs ORDER BY created_at DESC WITH PAYLOAD false LIMIT 10;")
            .unwrap();
    let op = plan(&stmt).unwrap();
    assert!(matches!(op, PlannedOperation::Scroll { .. }));
    assert_eq!(op.collection(), Some("docs"));
}
