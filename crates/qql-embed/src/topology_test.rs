//! Focused tests for `USING` kind resolution from collection topology.
//!
//! `resolve_query_vector_kinds` fills `VectorTarget.kind` (and omitted
//! targets) before embedding; `query_needs_kind_resolution` is the
//! executor fast-path predicate ("skip resolution when false").

use qql_core::ast::{
    PageSpec, QueryCollection, QueryExpr, QueryInput, QueryOutput, QueryStmt, Stmt, VectorKind,
    VectorValue,
};
use qql_core::parser::Parser;

use crate::topology::{TopologyNames, query_needs_kind_resolution, resolve_query_vector_kinds};

fn parse_query(sql: &str) -> QueryStmt {
    match Parser::parse(sql).unwrap() {
        Stmt::Query(query) => *query,
        other => panic!("expected query, got {other:?}"),
    }
}

fn topology(dense: &[&str], sparse: &[&str], multivector: &[&str]) -> TopologyNames {
    TopologyNames {
        dense: dense.iter().map(ToString::to_string).collect(),
        sparse: sparse.iter().map(ToString::to_string).collect(),
        multivector: multivector.iter().map(ToString::to_string).collect(),
    }
}

fn using_of(query: &QueryStmt) -> Option<(String, Option<VectorKind>, bool)> {
    let QueryExpr::Nearest { using, .. } = &query.expression else {
        panic!("expected Nearest, got {:?}", query.expression);
    };
    using.as_ref().map(|t| (t.name.clone(), t.kind, t.multi))
}

#[test]
fn unnamed_default_topology_leaves_using_absent() {
    let mut query = parse_query("QUERY TEXT 'x' FROM docs LIMIT 10");
    resolve_query_vector_kinds("docs", &mut query, &TopologyNames::default()).unwrap();
    assert!(using_of(&query).is_none());
}

#[test]
fn single_named_dense_fills_using() {
    let mut query = parse_query("QUERY TEXT 'x' FROM docs LIMIT 10");
    resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &[], &[])).unwrap();
    assert_eq!(
        using_of(&query),
        Some(("dense".to_string(), Some(VectorKind::Dense), false))
    );
}

#[test]
fn ambiguous_topology_requires_using() {
    let mut query = parse_query("QUERY TEXT 'x' FROM docs LIMIT 10");
    let err =
        resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &["sparse"], &[]))
            .unwrap_err();
    assert_eq!(err.code, "QQL-MISSING-USING");
}

#[test]
fn explicit_using_kind_filled_from_schema() {
    let mut query = parse_query("QUERY TEXT 'x' FROM docs USING sparse LIMIT 10");
    resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &["sparse"], &[]))
        .unwrap();
    assert_eq!(
        using_of(&query),
        Some(("sparse".to_string(), Some(VectorKind::Sparse), false))
    );
}

#[test]
fn using_kind_mismatch_rejected() {
    let mut query = parse_query("QUERY TEXT 'x' FROM docs USING dense AS SPARSE LIMIT 10");
    let err = resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &[], &[]))
        .unwrap_err();
    assert_eq!(err.code, "QQL-VECTOR-KIND");
}

#[test]
fn unknown_name_without_kind_fails_closed() {
    // C1 branch A: no declared kind → fail closed.
    let mut query = parse_query("QUERY TEXT 'x' FROM docs USING typo LIMIT 10");
    let err = resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &["s"], &[]))
        .unwrap_err();
    assert_eq!(err.code, "QQL-UNKNOWN-VECTOR");
    assert!(err.message.contains("typo"), "got: {}", err.message);
}

#[test]
fn unknown_name_with_kind_kept_for_offline() {
    // C1 branch B (deliberate carve-out): an already-declared AS kind is kept
    // so offline / empty-mock-schema queries resolve without a round trip.
    let mut query = parse_query("QUERY TEXT 'x' FROM docs USING typo AS DENSE LIMIT 10");
    resolve_query_vector_kinds("docs", &mut query, &TopologyNames::default()).unwrap();
    assert_eq!(
        using_of(&query),
        Some(("typo".to_string(), Some(VectorKind::Dense), false))
    );
}

#[test]
fn merge_dense_sparse_inputs_rejected() {
    let mut query = QueryStmt {
        ctes: Vec::new(),
        collection: QueryCollection::Inherited,
        expression: QueryExpr::Recommend {
            positive: vec![QueryInput::Vector(VectorValue::Dense(vec![1.0]))],
            negative: vec![QueryInput::Vector(VectorValue::Sparse {
                indices: vec![1],
                values: vec![1.0],
            })],
            strategy: None,
            using: None,
            prefetch: Vec::new(),
        },
        filter: None,
        params: None,
        score_threshold: None,
        group: None,
        output: QueryOutput::default(),
        page: PageSpec::default(),
        shard_key: None,
    };
    let err =
        resolve_query_vector_kinds("docs", &mut query, &topology(&["d"], &["s"], &[])).unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-VECTOR-KIND");
}

#[test]
fn hybrid_missing_sparse_name_errors() {
    let mut query = parse_query("QUERY HYBRID TEXT 'q' DENSE d FUSION RRF FROM docs LIMIT 10");
    let err = resolve_query_vector_kinds("docs", &mut query, &topology(&["d"], &["s1", "s2"], &[]))
        .unwrap_err();
    assert_eq!(err.code, "QQL-MISSING-USING");
}

#[test]
fn hybrid_unknown_vector_name_errors() {
    let mut query =
        parse_query("QUERY HYBRID TEXT 'q' DENSE nope SPARSE s FUSION RRF FROM docs LIMIT 10");
    let err =
        resolve_query_vector_kinds("docs", &mut query, &topology(&["d"], &["s"], &[])).unwrap_err();
    assert_eq!(err.code, "QQL-UNKNOWN-VECTOR");
}

#[test]
fn rerank_sparse_using_rejected() {
    let mut query = parse_query(
        "WITH c AS (QUERY TEXT 'x' USING dense AS DENSE LIMIT 100) \
         QUERY RERANK TEXT 'r' MODEL 'm' FROM docs USING s PREFETCH (c) LIMIT 10;",
    );
    let err = resolve_query_vector_kinds(
        "docs",
        &mut query,
        &topology(&["dense", "colbert"], &["s"], &["colbert"]),
    )
    .unwrap_err();
    assert_eq!(err.code, "QQL-VECTOR-KIND");
}

#[test]
fn rerank_without_dense_topology_errors() {
    // `USING` is parser-required for RERANK, so the hand-built AST exercises
    // the `select(Some(Dense)) → None` branch (typed, zero candidates).
    let mut query = QueryStmt {
        ctes: Vec::new(),
        collection: QueryCollection::Inherited,
        expression: QueryExpr::Rerank {
            input: QueryInput::Text {
                text: "r".to_string(),
                model: None,
                text_param: None,
                options: Vec::new(),
            },
            model: "m".to_string(),
            using: None,
            prefetch: Vec::new(),
        },
        filter: None,
        params: None,
        score_threshold: None,
        group: None,
        output: QueryOutput::default(),
        page: PageSpec::default(),
        shard_key: None,
    };
    let err =
        resolve_query_vector_kinds("docs", &mut query, &topology(&[], &["s"], &[])).unwrap_err();
    assert_eq!(err.code, "QQL-MISSING-USING");
}

#[test]
fn needs_kind_false_for_points_orderby_sample() {
    for sql in [
        "QUERY POINTS (42, 'uuid-1') FROM docs;",
        "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
        "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
    ] {
        let query = parse_query(sql);
        assert!(
            !query_needs_kind_resolution(&query),
            "{sql} must not need kind resolution"
        );
    }
}

#[test]
fn needs_kind_true_for_unresolved_nearest() {
    let query = parse_query("QUERY TEXT 'x' FROM docs USING sparse LIMIT 10");
    assert!(query_needs_kind_resolution(&query));
}

#[test]
fn needs_kind_dense_non_multi_stays_true_until_multi_set() {
    // C4 pin: kind-resolved dense targets still report true (the schema may
    // upgrade them to multivector), so the executor re-walks dense queries.
    // Only an explicit multi flag clears the predicate.
    let mut query = parse_query("QUERY TEXT 'x' FROM docs USING dense AS DENSE LIMIT 10");
    assert!(query_needs_kind_resolution(&query));
    resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &[], &[])).unwrap();
    assert!(query_needs_kind_resolution(&query));

    let resolved_multi = parse_query("QUERY TEXT 'x' FROM docs USING dense AS MULTI LIMIT 10");
    assert!(!query_needs_kind_resolution(&resolved_multi));
}

#[test]
fn needs_kind_reaches_into_ctes() {
    let query = parse_query(
        "WITH c AS (QUERY TEXT 'x' FROM docs USING sparse LIMIT 10) \
         QUERY POINTS (1) FROM docs;",
    );
    assert!(query_needs_kind_resolution(&query));
}

#[test]
fn schema_multivector_upgrades_dense_to_multi() {
    let mut query = parse_query("QUERY TEXT 'q' FROM docs USING colbert LIMIT 10");
    resolve_query_vector_kinds(
        "docs",
        &mut query,
        &topology(&["dense", "colbert"], &[], &["colbert"]),
    )
    .unwrap();
    assert_eq!(
        using_of(&query),
        Some(("colbert".to_string(), Some(VectorKind::Dense), true))
    );
}

#[test]
fn as_multi_on_sparse_rejected() {
    let mut query = parse_query("QUERY TEXT 'q' FROM docs USING sparse AS MULTI LIMIT 10");
    let err =
        resolve_query_vector_kinds("docs", &mut query, &topology(&["dense"], &["sparse"], &[]))
            .unwrap_err();
    assert_eq!(err.code, "QQL-VECTOR-KIND");
}
