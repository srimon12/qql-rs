use crate::ast::{ComparisonOp, FilterExpr, PointIdPredicate, Stmt};
use crate::parser::Parser;

fn filter_of(source: &str) -> Option<Box<FilterExpr>> {
    let s = Parser::parse(source).unwrap();
    match s {
        Stmt::Query(q) => q.filter,
        _ => panic!("expected query"),
    }
}

#[test]
fn simple_equality() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE status = 'active';").unwrap();
    assert!(matches!(*f, FilterExpr::Compare {
        field, op: ComparisonOp::Eq, ..
    } if field == "status"));
}

#[test]
fn not_equals_is_normalized() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE status != 'deleted';").unwrap();
    assert!(matches!(*f, FilterExpr::Not { .. }));
}

#[test]
fn range_operators() {
    for (op_str, op) in [
        (">", ComparisonOp::Gt),
        (">=", ComparisonOp::Gte),
        ("<", ComparisonOp::Lt),
        ("<=", ComparisonOp::Lte),
    ] {
        let source = format!("QUERY TEXT 'x' FROM docs WHERE count {} 5;", op_str);
        let f = filter_of(&source).unwrap();
        assert!(
            matches!(*f, FilterExpr::Compare { op: expected, .. } if expected == op),
            "failed for {}",
            op_str
        );
    }
}

#[test]
fn between() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE age BETWEEN 18 AND 65;").unwrap();
    assert!(matches!(*f, FilterExpr::Between { .. }));
}

#[test]
fn in_list() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE tag IN (1, 2, 3);").unwrap();
    assert!(matches!(*f, FilterExpr::In { .. }));
}

#[test]
fn not_in() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE tag NOT IN (4, 5);").unwrap();
    assert!(matches!(*f, FilterExpr::Not { .. }));
}

#[test]
fn is_null_and_not_null() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE description IS NULL;").unwrap();
    assert!(matches!(*f, FilterExpr::IsNull { .. }));

    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE description IS NOT NULL;").unwrap();
    assert!(matches!(*f, FilterExpr::Not { .. }));
}

#[test]
fn is_empty() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE tags IS EMPTY;").unwrap();
    assert!(matches!(*f, FilterExpr::IsEmpty { .. }));
}

#[test]
fn match_text() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE title MATCH 'search terms';").unwrap();
    assert!(matches!(*f, FilterExpr::MatchText { .. }));
}

#[test]
fn match_any() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE tags MATCH ANY ('rust', 'go');").unwrap();
    assert!(matches!(*f, FilterExpr::MatchAny { .. }));
}

#[test]
fn match_phrase() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE body MATCH PHRASE 'exact phrase';").unwrap();
    assert!(matches!(*f, FilterExpr::MatchPhrase { .. }));
}

#[test]
fn logical_and_or_not() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE a = 1 AND b = 2 OR c = 3;").unwrap();
    assert!(matches!(*f, FilterExpr::Or { .. }));

    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE a = 1 AND (b = 2 OR c = 3);").unwrap();
    assert!(matches!(*f, FilterExpr::And { .. }));

    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE NOT (a = 1 AND b = 2);").unwrap();
    assert!(matches!(*f, FilterExpr::Not { .. }));
}

#[test]
fn nested_filter() {
    let f =
        filter_of("QUERY TEXT 'x' FROM docs WHERE NESTED('comments', author = 'alice');").unwrap();
    assert!(matches!(*f, FilterExpr::Nested { .. }));
}

#[test]
fn has_vector() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE HAS_VECTOR dense;").unwrap();
    assert!(matches!(*f, FilterExpr::HasVector { .. }));
}

#[test]
fn values_count() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE tags VALUES_COUNT > 2;").unwrap();
    assert!(matches!(*f, FilterExpr::ValuesCount { .. }));
}

#[test]
fn geo_radius_filter() {
    let f = filter_of(
        "QUERY TEXT 'x' FROM docs WHERE location GEO_RADIUS {center: {lat: 52.5, lon: 13.4}, radius: 1000};"
    ).unwrap();
    assert!(matches!(*f, FilterExpr::GeoRadius { .. }));
}

#[test]
fn geo_bbox_filter() {
    let f = filter_of(
        "QUERY TEXT 'x' FROM docs WHERE area GEO_BBOX {top_left: {lat: 1, lon: 2}, bottom_right: {lat: 3, lon: 4}};"
    ).unwrap();
    assert!(matches!(*f, FilterExpr::GeoBoundingBox { .. }));
}

#[test]
fn id_predicate_simple() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE id = 42;").unwrap();
    assert!(matches!(*f, FilterExpr::PointId(PointIdPredicate::Eq(_))));
}

#[test]
fn id_predicate_in() {
    let f = filter_of("QUERY TEXT 'x' FROM docs WHERE id IN (1, 'uuid', 3);").unwrap();
    assert!(matches!(*f, FilterExpr::PointId(PointIdPredicate::In(ref ids)) if ids.len() == 3));
}
