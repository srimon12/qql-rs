//! Filter decoding round-trip invariants.

mod common;

use common::convert;

// ── Filters ─────────────────────────────────────────────────────

#[test]
fn filter_predicates_roundtrip() {
    let cases: Vec<(&str, &str)> = vec![
        (
            r#"{"must": [{"key": "a", "match": {"value": 1}}]}"#,
            "a = 1",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"value": false}}]}"#,
            "a = false",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"text": "full text"}}]}"#,
            "a MATCH 'full text'",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"phrase": "exact phrase"}}]}"#,
            "a MATCH PHRASE 'exact phrase'",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"prefix": "pre"}}]}"#,
            "a MATCH PREFIX 'pre'",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"any": ["x", "y"]}}]}"#,
            "a MATCH ANY ('x', 'y')",
        ),
        (
            r#"{"must": [{"key": "a", "match": {"any": [1, 2]}}]}"#,
            "a MATCH ANY (1, 2)",
        ),
        (
            r#"{"must": [{"key": "n", "range": {"gt": 1, "lt": 9}}]}"#,
            "n > 1 AND n < 9",
        ),
        (r#"{"must": [{"key": "n", "range": {"lt": 9}}]}"#, "n < 9"),
        (
            r#"{"must": [{"key": "d", "range": {"gte": "2024-01-01T00:00:00Z", "lte": "2024-12-31T00:00:00Z"}}]}"#,
            "d BETWEEN '2024-01-01T00:00:00Z' AND '2024-12-31T00:00:00Z'",
        ),
        (
            r#"{"must": [{"key": "tag", "values_count": {"gte": 2}}]}"#,
            "tag VALUES_COUNT >= 2",
        ),
        (
            r#"{"must": [{"key": "tag", "values_count": {"gte": 2, "lte": 2}}]}"#,
            "tag VALUES_COUNT = 2",
        ),
        (r#"{"must": [{"is_empty": {"key": "a"}}]}"#, "a IS EMPTY"),
        (r#"{"must": [{"is_null": {"key": "a"}}]}"#, "a IS NULL"),
        (r#"{"must": [{"has_id": [1, 2]}]}"#, "id IN (1, 2)"),
        (r#"{"must": [{"has_id": [1]}]}"#, "id = 1"),
        (r#"{"must": [{"has_vector": "dense"}]}"#, "HAS_VECTOR dense"),
        (
            r#"{"must": [{"slice": {"total": 4, "index": 2}}]}"#,
            "SLICE (4, 2)",
        ),
        (
            r#"{"must": [{"nested": {"key": "items", "filter": {"must": [{"key": "price", "range": {"gte": 10}}]}}}]}"#,
            "NESTED('items', price >= 10)",
        ),
        (
            r#"{"must": [{"key": "loc", "geo_bounding_box": {"top_left": {"lat": 52.5, "lon": 13.4}, "bottom_right": {"lat": 52.4, "lon": 13.5}}}]}"#,
            "loc GEO_BBOX {top_left: {lat: 52.5, lon: 13.4}, bottom_right: {lat: 52.4, lon: 13.5}}",
        ),
        (
            r#"{"must": [{"key": "loc", "geo_radius": {"center": {"lat": 52.5, "lon": 13.4}, "radius": 1000.0}}]}"#,
            "loc GEO_RADIUS {center: {lat: 52.5, lon: 13.4}, radius: 1000.0}",
        ),
        (
            r#"{"must": [{"key": "loc", "geo_polygon": {"exterior": {"points": [{"lat": 0.0, "lon": 0.0}, {"lat": 1.0, "lon": 0.0}, {"lat": 1.0, "lon": 1.0}]}, "interiors": [{"points": [{"lat": 0.1, "lon": 0.1}, {"lat": 0.2, "lon": 0.1}, {"lat": 0.2, "lon": 0.2}]}]}}]}"#,
            "loc GEO_POLYGON {exterior: [{lat: 0.0, lon: 0.0}, {lat: 1.0, lon: 0.0}, {lat: 1.0, lon: 1.0}], interiors: [[{lat: 0.1, lon: 0.1}, {lat: 0.2, lon: 0.1}, {lat: 0.2, lon: 0.2}]]}",
        ),
        (
            r#"{"must_not": [{"key": "a", "match": {"value": 1}}]}"#,
            "NOT a = 1",
        ),
        (
            r#"{"should": [{"key": "a", "match": {"value": 1}}, {"key": "b", "match": {"value": 2}}], "must": [{"key": "c", "match": {"value": 3}}]}"#,
            "c = 3 AND (a = 1 OR b = 2)",
        ),
        (
            r#"{"min_should": {"conditions": [{"key": "a", "match": {"value": 1}}, {"key": "b", "match": {"value": 2}}], "min_count": 2}}"#,
            "MIN SHOULD 2 (a = 1, b = 2)",
        ),
        (
            r#"{"must": [{"key": "title", "match": {"text_any": "red shoes"}}]}"#,
            "title MATCH TOKENS 'red shoes'",
        ),
        (
            r#"{"must": [{"key": "tags", "match": {"except": ["a", "b"]}}]}"#,
            "tags MATCH EXCEPT ('a', 'b')",
        ),
        (
            r#"{"must": [{"key": "code", "match": {"except": [1, 2]}}]}"#,
            "code MATCH EXCEPT (1, 2)",
        ),
        (
            r#"{"must": [{"key": "big", "match": {"value": 18446744073709551615}}]}"#,
            "big = 18446744073709551615",
        ),
        (
            r#"{"must": [{"key": "n", "range": {"gt": 18446744073709551615}}]}"#,
            "n > 18446744073709551615",
        ),
        (
            r#"{"must": [{"key": "name", "range": {"gt": "m"}}]}"#,
            "name > 'm'",
        ),
        (
            r#"{"must": [{"key": "name", "range": {"gte": "a", "lte": "m"}}]}"#,
            "name BETWEEN 'a' AND 'm'",
        ),
    ];
    for (filter, expected) in cases {
        let body = format!(r#"{{"filter": {filter}, "limit": 1}}"#);
        let stmts = convert(&body);
        assert!(
            stmts[0].contains(expected),
            "filter {filter} -> {} (expected to contain {expected})",
            stmts[0]
        );
    }
}

#[test]
fn geo_filters_decode_to_ast_not_errors() {
    // `geo_radius` and `geo_bounding_box` must come back as real predicates.
    let stmts = convert(
        r#"{"filter": {"must": [{"key": "loc", "geo_radius": {"center": {"lat": 1.5, "lon": 2.5}, "radius": 10.0}}]}, "limit": 1}"#,
    );
    assert!(stmts[0].contains("GEO_RADIUS"), "{}", stmts[0]);
}
