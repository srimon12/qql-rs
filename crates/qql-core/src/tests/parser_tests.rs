use crate::ast::{FusionMethod, QueryCollection, QueryExpr, QueryInput, Stmt};
use crate::parser::Parser;

#[test]
fn nearest_text_is_default_shorthand() {
    let s = Parser::parse("QUERY 'hello' FROM docs;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Text { ref text, model: None }, ..
    } if text == "hello"));
    assert_eq!(q.collection, QueryCollection::Explicit("docs".into()));
}

#[test]
fn nearest_explicit_text_with_model() {
    let s = Parser::parse("QUERY TEXT 'search' MODEL 'all-minilm' FROM docs;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Text { ref text, model: Some(ref m) }, ..
    } if text == "search" && m == "all-minilm"));
}

#[test]
fn nearest_vector() {
    let s = Parser::parse("QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Vector(_), using: Some(ref u), ..
    } if u == "dense"));
    assert_eq!(q.page.limit, Some(5));
}

#[test]
fn nearest_point() {
    let s = Parser::parse("QUERY NEAREST POINT 42 FROM docs USING dense;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Point(crate::ast::PointId::Number(42)), using: Some(ref u), ..
    } if u == "dense"));
}

#[test]
fn nearest_point_uuid() {
    let s = Parser::parse("QUERY NEAREST POINT 'abc-def' FROM docs USING dense;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Point(crate::ast::PointId::String(ref s)), ..
    } if s == "abc-def"));
}

#[test]
fn points_lookup() {
    let s = Parser::parse("QUERY POINTS (42, 'uuid-1') FROM docs WITH PAYLOAD true;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Points { ref ids } if ids.len() == 2));
}

#[test]
fn recommend_with_strategy() {
    let s = Parser::parse(
        "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY average_vector FROM docs USING dense;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Recommend { .. }));
}

#[test]
fn context_search() {
    let s = Parser::parse(
        "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2, POSITIVE POINT 3 NEGATIVE POINT 4) FROM docs;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Context { ref pairs, .. } if pairs.len() == 2));
}

#[test]
fn discover_search() {
    let s = Parser::parse(
        "QUERY DISCOVER TARGET POINT 1 CONTEXT (POSITIVE POINT 2 NEGATIVE POINT 3) FROM docs;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Discover { .. }));
}

#[test]
fn order_by() {
    let s = Parser::parse("QUERY ORDER BY created_at DESC FROM docs LIMIT 10;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::OrderBy { ref field, .. } if field == "created_at"));
}

#[test]
fn sample_random() {
    let s = Parser::parse("QUERY SAMPLE RANDOM FROM docs LIMIT 10;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::SampleRandom));
}

#[test]
fn fusion_with_prefetch() {
    let s = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert_eq!(q.ctes.len(), 2);
    assert!(matches!(q.expression, QueryExpr::Fusion { method: FusionMethod::Rrf, ref prefetch } if prefetch.len() == 2));
}

#[test]
fn fusion_dbsf() {
    let s = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY FUSION DBSF FROM docs PREFETCH (d) LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Fusion { method: FusionMethod::Dbsf, .. }));
}

#[test]
fn formula_query() {
    let s = Parser::parse(
        "QUERY FORMULA $score + 1 DEFAULTS (missing = 0) FROM docs;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Formula { .. }));
}

#[test]
fn relevance_feedback() {
    let s = Parser::parse(
        "QUERY RELEVANCE FEEDBACK TARGET POINT 1 FEEDBACK ((POINT 2, 0.8)) STRATEGY naive (a = 1, b = 0.5, c = 0.25) FROM docs;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::RelevanceFeedback { .. }));
}

#[test]
fn mmr_query() {
    let s = Parser::parse(
        "QUERY MMR TEXT 'diverse' DIVERSITY 0.4 CANDIDATES 50 FROM docs USING dense;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Mmr {
        input: QueryInput::Text { ref text, .. }, diversity, candidates, ..
    } if text == "diverse" && (diversity - 0.4).abs() < 1e-10 && candidates == 50));
}

#[test]
fn mmr_diversity_must_be_valid() {
    assert!(Parser::parse("QUERY MMR TEXT 'x' DIVERSITY 1.5 CANDIDATES 10 FROM docs;").is_err());
    assert!(Parser::parse("QUERY MMR TEXT 'x' DIVERSITY -0.1 CANDIDATES 10 FROM docs;").is_err());
    assert!(Parser::parse("QUERY MMR TEXT 'x' DIVERSITY 0.5 FROM docs;").is_err());
}

#[test]
fn hybrid_shorthand() {
    let s = Parser::parse(
        "QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Hybrid {
        ref text, fusion: FusionMethod::Rrf, ..
    } if text == "search"));
}

#[test]
fn rerank_query() {
    let s = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY RERANK TEXT 'x' MODEL 'reranker' FROM docs USING colbert PREFETCH (c) LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Rerank {
        ref model, ref using, ..
    } if model == "reranker" && using == "colbert"));
}

#[test]
fn query_clauses_full_order() {
    let s = Parser::parse(
        "QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2;",
    ).unwrap();
    assert!(matches!(s, Stmt::Query(_)));
}

#[test]
fn select_is_rejected() {
    assert!(Parser::parse("SELECT * FROM docs WHERE id = 42").is_err());
}

#[test]
fn numeric_literal_as_query_is_rejected() {
    assert!(Parser::parse("QUERY 42 FROM docs").is_err());
}

#[test]
fn trailing_semicolons_rejected() {
    assert!(Parser::parse_all("SHOW COLLECTIONS;; SHOW COLLECTION docs").is_err());
}

#[test]
fn parse_all_semicolons_required() {
    assert_eq!(
        Parser::parse_all("SHOW COLLECTIONS; SHOW COLLECTION docs;").unwrap().len(),
        2
    );
    assert!(Parser::parse_all("SHOW COLLECTIONS SHOW COLLECTION docs").is_err());
}
