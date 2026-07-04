use alloc::boxed::Box;
use alloc::vec;

use crate::ast::{FilterExpr, FormulaExpr, QueryMode, QueryType, Stmt};
use crate::parser_test::{assert_parse_ok, i64_val, str_val};

#[test]
fn test_query_with_lookup() {
    let stmt =
            assert_parse_ok("QUERY 'search' FROM docs LIMIT 10 GROUP BY 'category' GROUP_SIZE 5 WITH LOOKUP FROM metadata");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some("docs"));
            assert_eq!(q.limit, 10);
            assert_eq!(q.group_by, Some("category"));
            assert_eq!(q.group_size, Some(5));
            assert_eq!(q.with_lookup_collection, Some("metadata"));
        }
        _ => panic!("expected Query stmt"),
    }
}

// ── Query: Nested filter ─────────────────────────────────────

#[test]
fn test_query_nested_filter() {
    let stmt = assert_parse_ok(
            "QUERY 'pricing' FROM content LIMIT 5 WHERE branch = 'root' AND NOT NESTED('overwritten_in', by = 'root' AND seq <= 2)",
        );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some("content"));
            let filter = q.query_filter.as_ref().unwrap();
            match filter.as_ref() {
                FilterExpr::And { operands } => {
                    assert_eq!(operands.len(), 2);
                    assert_eq!(
                        operands[0],
                        FilterExpr::Compare {
                            field: "branch",
                            op: "=",
                            value: str_val("root"),
                        }
                    );
                    match &operands[1] {
                        FilterExpr::Not { operand } => match operand.as_ref() {
                            FilterExpr::Nested {
                                path,
                                filter: inner,
                            } => {
                                assert_eq!(*path, "overwritten_in");
                                match inner.as_ref() {
                                    FilterExpr::And {
                                        operands: inner_ops,
                                    } => {
                                        assert_eq!(inner_ops.len(), 2);
                                        assert_eq!(
                                            inner_ops[0],
                                            FilterExpr::Compare {
                                                field: "by",
                                                op: "=",
                                                value: str_val("root"),
                                            }
                                        );
                                        assert_eq!(
                                            inner_ops[1],
                                            FilterExpr::Compare {
                                                field: "seq",
                                                op: "<=",
                                                value: i64_val(2),
                                            }
                                        );
                                    }
                                    _ => panic!("expected inner And"),
                                }
                            }
                            _ => panic!("expected Nested"),
                        },
                        _ => panic!("expected Not"),
                    }
                }
                _ => panic!("expected And"),
            }
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_nested_filter_simple() {
    let stmt =
        assert_parse_ok("QUERY 'test' FROM docs LIMIT 5 WHERE NESTED('tags', name = 'important')");
    match stmt {
        Stmt::Query(q) => {
            let filter = q.query_filter.as_ref().unwrap();
            match filter.as_ref() {
                FilterExpr::Nested {
                    path,
                    filter: inner,
                } => {
                    assert_eq!(*path, "tags");
                    match inner.as_ref() {
                        FilterExpr::Compare { field, op, value } => {
                            assert_eq!(*field, "name");
                            assert_eq!(*op, "=");
                            assert_eq!(*value, str_val("important"));
                        }
                        _ => panic!("expected Compare"),
                    }
                }
                _ => panic!("expected Nested"),
            }
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_parse_query() {
    let stmt = assert_parse_ok("QUERY 'test' FROM docs LIMIT 5");
    match stmt {
        Stmt::Query(_) => {}
        _ => panic!("expected Query"),
    }
}

// ── Merge duplicate WITH clause ──────────────────────────────

#[test]
fn test_merge_duplicate_with_clause() {
    let stmt = assert_parse_ok(
        "QUERY NEAREST 'text' FROM test LIMIT 10 WITH (exact = true) WITH (acorn = true)",
    );
    match stmt {
        Stmt::Query(q) => {
            let wc = q.with_clause.as_ref().unwrap();
            assert!(wc.exact);
            assert!(wc.acorn);
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: HYBRID / SPARSE / DENSE ──────────────────────────

#[test]
fn test_query_hybrid() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 5 USING HYBRID");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.query_type, QueryType::Hybrid);
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_query_sparse() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 5 USING SPARSE");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.query_type, QueryType::Sparse);
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_query_dense() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 5 USING DENSE");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.query_type, QueryType::Dense);
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: BOOST ─────────────────────────────────────────────

#[test]
fn test_query_boost() {
    let stmt = assert_parse_ok("QUERY 'test' FROM docs LIMIT 10 BOOST ($score * 2)");
    match stmt {
        Stmt::Query(q) => {
            assert!(q.formula.is_some());
            match q.formula.as_ref().unwrap().as_ref() {
                FormulaExpr::Mul { left, right } => {
                    match left.as_ref() {
                        FormulaExpr::Variable { name } => assert_eq!(*name, "$score"),
                        _ => panic!("expected Variable"),
                    }
                    match right.as_ref() {
                        FormulaExpr::Constant { value } => assert_eq!(*value, 2.0),
                        _ => panic!("expected Constant"),
                    }
                }
                _ => panic!("expected Mul"),
            }
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: SCORE THRESHOLD / WHERE / GROUP BY ────────────────

#[test]
fn test_query_where() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 10 WHERE field = 'value'");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(
                q.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "field",
                    op: "=",
                    value: str_val("value"),
                }))
            );
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_query_score_threshold() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 10 SCORE THRESHOLD 0.5");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.score_threshold, Some(0.5));
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_query_group_by() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 10 GROUP BY 'category'");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.group_by, Some("category"));
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: LOOKUP FROM ───────────────────────────────────────

#[test]
fn test_query_lookup() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 10 LOOKUP FROM metadata");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.lookup_from, Some("metadata"));
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: RERANK ────────────────────────────────────────────

#[test]
fn test_query_rerank() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs LIMIT 10 RERANK");
    match stmt {
        Stmt::Query(q) => {
            assert!(q.rerank);
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: RELEVANCE FEEDBACK ────────────────────────────────

#[test]
fn test_query_relevance_feedback() {
    let stmt = assert_parse_ok(
        "QUERY RELEVANCE FEEDBACK TARGET 'example' FEEDBACK (('pos1', 1.0), ('neg1', 0.0))",
    );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::RelevanceFeedback);
            assert_eq!(q.feedback_target, Some(str_val("example")));
            assert_eq!(q.feedback_items.len(), 2);
            assert_eq!(q.feedback_items[0].example, str_val("pos1"));
            assert_eq!(q.feedback_items[0].score, 1.0);
            assert_eq!(q.feedback_items[1].example, str_val("neg1"));
            assert_eq!(q.feedback_items[1].score, 0.0);
        }
        _ => panic!("expected Query"),
    }
}

// ── Query: Payload/Vectors ───────────────────────────────────

#[test]
fn test_query_payload_vectors() {
    let stmt = assert_parse_ok(
            "QUERY 'search' FROM docs WITH PAYLOAD (include = ['title'], exclude = ['metadata']) WITH VECTORS true",
        );
    match stmt {
        Stmt::Query(q) => {
            let wp = q.with_payload.as_ref().unwrap();
            assert_eq!(wp.include, vec!["title"]);
            assert_eq!(wp.exclude, vec!["metadata"]);
        }
        _ => panic!("expected Query"),
    }
}

// ── Edge cases ───────────────────────────────────────────────

#[test]
fn test_parse_edge_cases() {
    assert_parse_ok("QUERY 'text' FROM test LIMIT 10 WITH (exact = true) WITH (acorn = true)");
}

#[test]
fn test_query_with_cte_fusion_from() {
    let stmt = assert_parse_ok("WITH cte AS (QUERY 'a' LIMIT 10) FUSION RRF FROM docs");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.ctes.len(), 1);
            assert_eq!(q.fusion_type, Some("RRF"));
            assert_eq!(q.collection, Some("docs"));
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_fusion_from_keyword() {
    let stmt = assert_parse_ok("QUERY 'text' FROM docs FUSION RRF");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.fusion_type, Some("RRF"));
            assert_eq!(q.collection, Some("docs"));
        }
        _ => panic!("expected Query"),
    }
}

#[test]
fn test_parse_all_statements() {
    let script = "
        CREATE COLLECTION my_col HYBRID;
        INSERT INTO my_col VALUES {id: 1, text: 'hello'};
        QUERY 'hello' FROM my_col;
    ";
    let stmts = crate::parser::Parser::parse_all(script).unwrap();
    assert_eq!(stmts.len(), 3);
    assert!(matches!(stmts[0], Stmt::CreateCollection(_)));
    assert!(matches!(stmts[1], Stmt::Insert(_)));
    assert!(matches!(stmts[2], Stmt::Query(_)));
}
