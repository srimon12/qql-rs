//! Typed result accessors: `hits()` serves the normalization cache without
//! a per-call JSON clone + parse, while `hits_json()`/`ids()`/`count()`/
//! `facet()` keep their borrowed JSON behavior and the wire shape stays
//! byte-identical (`typed_hits` is serde-skipped).

use crate::executor::{ExecResponse, SearchHit};

fn hits_data() -> serde_json::Value {
    serde_json::json!([
        {"id": 1, "score": 0.9, "payload": {"text": "hello"}},
        {"id": 2, "score": 0.5, "payload": {"tag": "x"}},
    ])
}

fn lazy_resp() -> ExecResponse {
    ExecResponse {
        ok: true,
        operation: "QUERY".to_string(),
        message: "Found 2 hits".to_string(),
        data: Some(hits_data()),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    }
}

#[test]
fn hits_parses_once_and_matches_json_view() {
    let resp = lazy_resp();
    let typed = resp.hits().expect("hits data must parse");
    assert_eq!(typed.len(), 2);
    assert_eq!(typed[0].score, 0.9);
    // `text` is an extractor-populated convenience field, not derived on
    // deserialize (pre-existing `hits()` behavior); payload carries through.
    assert_eq!(typed[0].text, None);
    assert_eq!(
        typed[0].payload.as_ref().and_then(|p| p.get("text")),
        Some(&serde_json::json!("hello"))
    );
    // Second call serves the cache: same result, no re-parse observable
    // through behavior (initialized exactly once).
    let again = resp.hits().expect("cached hits must serve");
    assert_eq!(again.len(), 2);
    assert_eq!(again[1].id, typed[1].id);
    // Borrowed JSON view is untouched and consistent.
    let raw = resp.hits_json().expect("hits_json must borrow");
    assert_eq!(raw.len(), 2);
    assert_eq!(raw[0]["id"], serde_json::json!(1));
    assert_eq!(resp.ids(), vec![1, 2]);
}

#[test]
fn eager_cache_serves_without_json() {
    // Normalization attaches the structs it already built; `hits()` must
    // return them even if `data` were absent.
    let hit = SearchHit {
        id: qql_plan::PlanPointId::Number(7),
        score: 1.0,
        text: None,
        payload: None,
        collection: None,
        vector: None,
    };
    let resp = ExecResponse {
        ok: true,
        operation: "QUERY".to_string(),
        message: "Found 1 hits".to_string(),
        data: None,
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    }
    .with_typed_hits(vec![hit]);
    let typed = resp.hits().expect("eager cache must serve");
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].score, 1.0);
}

#[test]
fn non_hits_payloads_still_yield_none() {
    for data in [
        serde_json::json!({"count": 3}),
        serde_json::json!([{"value": "books", "count": 15}]),
        serde_json::json!({"result": {"status": "completed"}}),
    ] {
        let resp = ExecResponse {
            ok: true,
            operation: "X".to_string(),
            message: "ok".to_string(),
            data: Some(data),
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        };
        assert!(resp.hits().is_none());
    }
    // …while their own accessors keep working off borrowed JSON.
    let count = ExecResponse {
        ok: true,
        operation: "COUNT".to_string(),
        message: "Count: 3".to_string(),
        data: Some(serde_json::json!({"count": 3})),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    };
    assert_eq!(count.count(), Some(3));
    let facet = ExecResponse {
        ok: true,
        operation: "FACET".to_string(),
        message: "Found 1 facet hit(s)".to_string(),
        data: Some(serde_json::json!([{"value": "books", "count": 15}])),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    };
    assert_eq!(facet.facet(), Some(vec![(serde_json::json!("books"), 15)]));
}

#[test]
fn typed_cache_is_wire_invisible() {
    let resp = lazy_resp();
    let _ = resp.hits(); // warm the cache: serialization must not observe it
    let val = serde_json::to_value(&resp).unwrap();
    assert!(
        !val.as_object().unwrap().contains_key("typed_hits"),
        "cache must stay off the wire"
    );
    // Old payloads (no cache key) still deserialize.
    let back: ExecResponse = serde_json::from_value(val).unwrap();
    assert_eq!(back.hits().unwrap().len(), 2);
}
