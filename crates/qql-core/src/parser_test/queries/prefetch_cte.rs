use alloc::vec;

use crate::ast::Stmt;
use crate::parser_test::assert_parse_ok;

// ── Query: Prefetch ──────────────────────────────────────────

#[test]
fn test_query_prefetch() {
    let input = "WITH p1 AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech' SCORE THRESHOLD 0.8), p2 AS (QUERY 'search' USING sparse LIMIT 100 WITH (exact = true))
QUERY 'search' FROM docs LIMIT 10 PREFETCH (p1, p2) FUSION RRF WITH (rrf_k = 10, rrf_weights = [0.7, 0.3])";
    let stmt = assert_parse_ok(input);
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some("docs"));
            assert_eq!(q.limit, 10);
            assert_eq!(q.ctes.len(), 2);
            assert_eq!(q.ctes[0].name, "p1");
            assert_eq!(q.ctes[1].name, "p2");
            assert_eq!(q.prefetch_refs.len(), 2);
            assert_eq!(q.prefetch_refs[0].cte_name, "p1");
            assert_eq!(q.prefetch_refs[1].cte_name, "p2");
            assert_eq!(q.fusion_type, Some("RRF"));
            let wc = q.with_clause.as_ref().unwrap();
            assert_eq!(wc.rrf_k, Some(10));
            assert_eq!(wc.rrf_weights, vec![0.7f32, 0.3f32]);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_case_insensitive() {
    let stmt = assert_parse_ok(
        "WITH MyCte AS (QUERY 'search' USING dense LIMIT 100)
QUERY 'search' FROM docs LIMIT 10 PREFETCH (mycte)",
    );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.ctes.len(), 1);
            assert_eq!(q.ctes[0].name, "MyCte");
            assert_eq!(q.prefetch_refs.len(), 1);
            assert_eq!(q.prefetch_refs[0].cte_name, "mycte");
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_fusion_without_prefetch() {
    let stmt = assert_parse_ok("QUERY 'test' FROM docs FUSION RRF");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.fusion_type, Some("RRF"));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_empty_prefetch_block() {
    let stmt = assert_parse_ok("QUERY 'test' FROM docs PREFETCH ()");
    match stmt {
        Stmt::Query(_) => {}
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_duplicate_fusion() {
    // Rust parser silently ignores duplicate FUSION, uses the first one
    let stmt = assert_parse_ok("QUERY 'test' FROM docs USING HYBRID FUSION RRF FUSION DBSF");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.fusion_type, Some("RRF"));
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_query_prefetch_nested_cte() {
    let stmt = assert_parse_ok(
            "WITH p1 AS (QUERY 'inner' USING dense LIMIT 50), p2 AS (QUERY 'outer' USING sparse LIMIT 100 PREFETCH (p1)) QUERY 'test' FROM docs PREFETCH (p2)",
        );
    match stmt {
        Stmt::Query(_) => {}
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_fusion_dbsf() {
    let stmt = assert_parse_ok("QUERY 'test' FROM docs USING HYBRID FUSION DBSF");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.fusion_type, Some("DBSF"));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_cte_with_recommend() {
    let stmt = assert_parse_ok(
            "WITH p1 AS (QUERY RECOMMEND WITH (positive = (1, 2), negative = (3)) USING dense) QUERY 'test' FROM docs PREFETCH (p1)",
        );
    match stmt {
        Stmt::Query(_) => {}
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_per_ref_filter() {
    let input = "WITH a AS (QUERY 'search' USING dense LIMIT 100), b AS (QUERY 'search' USING sparse LIMIT 100)
QUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE category = 'tech', b SCORE THRESHOLD 0.5) FUSION RRF";
    let stmt = assert_parse_ok(input);
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.prefetch_refs.len(), 2);
            assert_eq!(q.prefetch_refs[0].cte_name, "a");
            assert!(q.prefetch_refs[0].filter.is_some());
            assert!(q.prefetch_refs[0].score_threshold.is_none());
            assert_eq!(q.prefetch_refs[1].cte_name, "b");
            assert!(q.prefetch_refs[1].filter.is_none());
            assert_eq!(q.prefetch_refs[1].score_threshold, Some(0.5));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_per_ref_both() {
    let input = "WITH a AS (QUERY 'search' USING dense LIMIT 100)
QUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8) FUSION RRF";
    let stmt = assert_parse_ok(input);
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.prefetch_refs.len(), 1);
            assert_eq!(q.prefetch_refs[0].cte_name, "a");
            assert!(q.prefetch_refs[0].filter.is_some());
            assert_eq!(q.prefetch_refs[0].score_threshold, Some(0.8));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_prefetch_per_ref_lookup() {
    let input = "WITH a AS (QUERY 'search' USING dense LIMIT 100)
QUERY 'search' FROM docs LIMIT 10 PREFETCH (a LOOKUP FROM external_col VECTOR 'dense_vec') FUSION RRF";
    let stmt = assert_parse_ok(input);
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.prefetch_refs.len(), 1);
            assert_eq!(q.prefetch_refs[0].cte_name, "a");
            assert_eq!(q.prefetch_refs[0].lookup_from, Some("external_col"));
            assert_eq!(q.prefetch_refs[0].lookup_vector, Some("dense_vec"));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_cte_query_raw_vector() {
    let stmt = assert_parse_ok(
            "WITH _pf0 AS (QUERY [0.5, 0.6] LIMIT 100) QUERY 'search' FROM docs LIMIT 10 PREFETCH (_pf0)",
        );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.ctes.len(), 1);
            assert_eq!(q.ctes[0].name, "_pf0");
            assert_eq!(q.ctes[0].stmt.raw_vector, vec![0.5, 0.6]);
        }
        _ => panic!("expected Query stmt"),
    }
}
