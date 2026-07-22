use crate::error::ErrorKind;
use crate::parser::Parser;

macro_rules! assert_parse_err {
    ($source:expr, $kind:ident) => {{
        let err = Parser::parse($source).expect_err(&format!("expected error for: {}", $source));
        assert_eq!(
            err.kind,
            ErrorKind::Parse,
            "wrong error kind for: {}",
            $source
        );
    }};
}

macro_rules! assert_validation_err {
    ($source:expr) => {{
        let err = Parser::parse($source).expect_err(&format!("expected error for: {}", $source));
        assert_eq!(
            err.kind,
            ErrorKind::Validation,
            "wrong error kind for: {}",
            $source
        );
    }};
}

#[test]
fn top_level_query_requires_from() {
    assert!(Parser::parse("QUERY USING dense;").is_err());
}

#[test]
fn clause_ordering_violations() {
    let invalid = [
        "QUERY TEXT 'x' FROM docs LIMIT 10 WHERE active = true;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 LIMIT 20;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 OFFSET 5 LIMIT 5;",
        "QUERY TEXT 'x' FROM docs LIMIT 10 WHERE x = 1 LIMIT 5;",
    ];
    for source in invalid {
        let err = Parser::parse(source)
            .expect_err(&format!("expected clause order error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Parse);
    }
}

#[test]
fn bare_using_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs USING;", Parse);
}

#[test]
fn bare_score_threshold_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs SCORE THRESHOLD;", Parse);
}

#[test]
fn top_level_query_without_from_rejected() {
    assert_validation_err!("QUERY TEXT 'x' USING dense;");
}

#[test]
fn generic_with_clause_rejected() {
    assert_parse_err!("QUERY TEXT 'x' FROM docs WITH (exact = true);", Parse);
}

#[test]
fn incomplete_mmr_rejected() {
    assert!(Parser::parse("QUERY MMR TEXT 'x' DIVERSITY 0.5 FROM docs;").is_err());
}

#[test]
fn fusion_without_prefetch_rejected() {
    assert_validation_err!("QUERY FUSION RRF FROM docs;");
}

#[test]
fn fusion_with_missing_prefetch_rejected() {
    assert_validation_err!("QUERY FUSION RRF FROM docs PREFETCH (missing);");
}

#[test]
fn rerank_without_prefetch_rejected() {
    let err = Parser::parse("QUERY RERANK TEXT 'x' MODEL 'm' FROM docs USING colbert;")
        .expect_err("should fail");
    assert_eq!(err.kind, ErrorKind::Validation);
}

#[test]
fn duplicate_keys_rejected() {
    let cases = [
        "UPSERT INTO docs VALUES {id: 1, Title: 'a', title: 'b'};",
        "CREATE COLLECTION docs WITH HNSW (m = 8, M = 16);",
        "CREATE INDEX ON COLLECTION docs FOR title WITH (on_disk = true, ON_DISK = false);",
    ];
    for source in cases {
        let err = Parser::parse(source)
            .expect_err(&format!("expected duplicate key error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Parse);
    }
}

#[test]
fn invalid_geo_rejected() {
    let cases = [
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 91, lon: 13}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 1}, radius: 1};",
        "QUERY TEXT 'x' FROM docs WHERE loc GEO_RADIUS {center: {lat: 1, lon: 2}, radius: 0};",
    ];
    for source in cases {
        let err = Parser::parse(source).expect_err(&format!("expected geo error for: {}", source));
        assert_eq!(err.kind, ErrorKind::Validation);
    }
}

#[test]
fn id_predicate_inequality_rejected() {
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE id > 4").is_err());
}

#[test]
fn empty_in_list_rejected() {
    assert_parse_err!("QUERY POINTS (1) FROM docs WHERE tag IN ();", Parse);
    assert_parse_err!("QUERY POINTS (1) FROM docs WHERE tag NOT IN ();", Parse);
}

#[test]
fn invalid_shard_params_rejected() {
    assert_parse_err!(
        "CREATE COLLECTION docs VECTORS (dense size=4 distance=Cosine) WITH PARAMS (sharding_method = true);",
        Parse
    );
    assert_parse_err!(
        "CREATE COLLECTION docs VECTORS (dense size=4 distance=Cosine) WITH PARAMS (shard_keys = [\"a\", 42]);",
        Parse
    );
    assert_parse_err!(
        "CREATE COLLECTION docs VECTORS (dense size=4 distance=Cosine) WITH PARAMS (shard_number = true);",
        Parse
    );
}

#[test]
fn match_any_requires_list() {
    assert!(Parser::parse("QUERY TEXT 'x' FROM docs WHERE tags MATCH ANY 'x y'").is_err());
}

#[test]
fn empty_prefetch_rejected() {
    let err = Parser::parse("WITH d AS (QUERY TEXT 'x' USING d LIMIT 10) QUERY FUSION RRF FROM docs PREFETCH () LIMIT 10;")
        .expect_err("empty prefetch should fail");
    assert_eq!(err.kind, ErrorKind::Parse);
}

#[test]
fn unknown_cte_rejected() {
    let err = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (nonexistent) LIMIT 10;",
    ).expect_err("unknown CTE should fail");
    assert_eq!(err.kind, ErrorKind::Validation);
}

#[test]
fn duplicate_cte_name_rejected() {
    let err = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' LIMIT 10), d AS (QUERY TEXT 'y' LIMIT 10) QUERY TEXT 'z' FROM docs;",
    ).expect_err("duplicate CTE should fail");
    assert_eq!(err.kind, ErrorKind::Parse);
}

#[test]
fn semicolon_script_separation() {
    assert!(Parser::parse_all("; SHOW COLLECTIONS").is_err());
    assert!(Parser::parse_all("SHOW COLLECTIONS;; SHOW COLLECTION docs").is_err());
}

#[test]
fn trailing_tokens_rejected() {
    assert!(Parser::parse("SHOW COLLECTIONS extra;").is_err());
    assert!(Parser::parse("SHOW COLLECTIONS FROM").is_err());
}
