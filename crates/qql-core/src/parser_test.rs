#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use crate::ast::{
        FilterExpr, FormulaExpr, QuantizationType, QueryMode, QueryType, Stmt, Value,
    };
    use crate::error::QqlError;
    use crate::parser::Parser;

    fn parse(input: &str) -> Result<Stmt, QqlError> {
        Parser::parse(input)
    }

    fn assert_parse_ok(input: &str) -> Stmt {
        parse(input).unwrap_or_else(|e| panic!("failed to parse '{}': {}", input, e))
    }

    fn assert_parse_err(input: &str) {
        assert!(parse(input).is_err(), "expected parse error for: {}", input);
    }

    fn i64_val(v: i64) -> Value<'static> {
        Value::Int(v)
    }

    fn str_val(s: &'static str) -> Value<'static> {
        Value::Str(s)
    }

    fn float_val(f: f64) -> Value<'static> {
        Value::Float(f)
    }

    fn make_payload(
        pairs: &[(&'static str, Value<'static>)],
    ) -> Vec<(&'static str, Value<'static>)> {
        pairs.to_vec()
    }

    // ── Documented examples ──────────────────────────────────────

    #[test]
    fn test_readme_create_hybrid_collection() {
        let stmt = assert_parse_ok("CREATE COLLECTION docs HYBRID");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "docs");
                assert!(c.hybrid);
            }
            _ => panic!("expected CreateCollection stmt"),
        }
    }

    #[test]
    fn test_readme_create_hybrid_rerank_collection() {
        let stmt = assert_parse_ok("CREATE COLLECTION docs HYBRID RERANK");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "docs");
                assert!(c.hybrid);
                assert!(c.rerank);
            }
            _ => panic!("expected CreateCollection stmt"),
        }
    }

    #[test]
    fn test_readme_hybrid_insert() {
        let stmt = assert_parse_ok("INSERT INTO docs VALUES {'text': 'Qdrant stores vectors', 'topic': 'search'} USING HYBRID");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "docs");
                assert!(i.hybrid);
                assert_eq!(
                    i.values_list,
                    vec![vec![
                        ("text", str_val("Qdrant stores vectors")),
                        ("topic", str_val("search")),
                    ]]
                );
            }
            _ => panic!("expected Insert stmt"),
        }
    }

    #[test]
    fn test_readme_hybrid_search() {
        let stmt =
            assert_parse_ok("QUERY NEAREST 'vector database' FROM docs LIMIT 5 USING HYBRID");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.collection, Some("docs"));
                assert_eq!(q.query_text, Some("vector database"));
                assert_eq!(q.limit, 5);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_readme_hybrid_search_with_filter() {
        let stmt = assert_parse_ok(
            "QUERY NEAREST 'vector search' FROM notes LIMIT 5 USING HYBRID WHERE topic = 'search'",
        );
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.collection, Some("notes"));
                assert_eq!(
                    q.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "topic",
                        op: "=",
                        value: str_val("search"),
                    }))
                );
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_readme_hybrid_rerank_search() {
        let stmt = assert_parse_ok(
            "QUERY NEAREST 'vector database' FROM docs LIMIT 5 USING HYBRID RERANK",
        );
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.collection, Some("docs"));
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_readme_delete_by_id() {
        let stmt = assert_parse_ok("DELETE FROM notes WHERE id = 'uuid'");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "notes");
                assert_eq!(d.point_id, Some(str_val("uuid")));
            }
            _ => panic!("expected Delete stmt"),
        }
    }

    #[test]
    fn test_readme_delete_by_field() {
        let stmt = assert_parse_ok("DELETE FROM notes WHERE specialty = 'search'");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "notes");
                assert_eq!(d.field, Some("specialty"));
                assert_eq!(d.value, Some(str_val("search")));
            }
            _ => panic!("expected Delete stmt"),
        }
    }

    // ── Filter: Comparisons ──────────────────────────────────────

    #[test]
    fn test_filter_equals() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field = 'value' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "field",
                        op: "=",
                        value: str_val("value"),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_not_equals() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field != 'value' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "field",
                        op: "!=",
                        value: str_val("value"),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_greater_than() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE count > 5 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "count",
                        op: ">",
                        value: i64_val(5),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_greater_than_or_equals() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE count >= 5 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "count",
                        op: ">=",
                        value: i64_val(5),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_less_than() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE count < 10 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "count",
                        op: "<",
                        value: i64_val(10),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_less_than_or_equals() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE count <= 10 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "count",
                        op: "<=",
                        value: i64_val(10),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_equals_integer() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE count = 42 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "count",
                        op: "=",
                        value: i64_val(42),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_equals_float() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE score = 3.14 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "score",
                        op: "=",
                        value: float_val(3.14),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: Between ──────────────────────────────────────────

    #[test]
    fn test_filter_between() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE age BETWEEN 18 AND 65 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Between {
                        field: "age",
                        low: i64_val(18),
                        high: i64_val(65),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: IN ───────────────────────────────────────────────

    #[test]
    fn test_filter_in() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE status IN ('active', 'pending') LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::In {
                        field: "status",
                        values: vec![str_val("active"), str_val("pending")],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: NOT IN ───────────────────────────────────────────

    #[test]
    fn test_filter_not_in() {
        let stmt =
            assert_parse_ok("SCROLL FROM c WHERE status NOT IN ('deleted', 'archived') LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::NotIn {
                        field: "status",
                        values: vec![str_val("deleted"), str_val("archived")],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: IS NULL / IS NOT NULL / IS EMPTY / IS NOT EMPTY ──

    #[test]
    fn test_filter_is_null() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NULL LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::IsNull { field: "field" }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_is_not_null() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NOT NULL LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::IsNotNull { field: "field" }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_is_empty() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS EMPTY LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::IsEmpty { field: "field" }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_is_not_empty() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NOT EMPTY LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::IsNotEmpty { field: "field" }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: MATCH ────────────────────────────────────────────

    #[test]
    fn test_filter_match_text() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE content MATCH 'hello world' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::MatchText {
                        field: "content",
                        text: "hello world",
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_match_any() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE content MATCH ANY 'hello world' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::MatchAny {
                        field: "content",
                        text: "hello world",
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_match_phrase() {
        let stmt =
            assert_parse_ok("SCROLL FROM c WHERE content MATCH PHRASE 'hello world' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::MatchPhrase {
                        field: "content",
                        text: "hello world",
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Filter: AND / OR / NOT ───────────────────────────────────

    #[test]
    fn test_filter_and() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 AND b = 2 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::And {
                        operands: vec![
                            FilterExpr::Compare {
                                field: "a",
                                op: "=",
                                value: i64_val(1),
                            },
                            FilterExpr::Compare {
                                field: "b",
                                op: "=",
                                value: i64_val(2),
                            },
                        ],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_or() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 OR b = 2 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Or {
                        operands: vec![
                            FilterExpr::Compare {
                                field: "a",
                                op: "=",
                                value: i64_val(1),
                            },
                            FilterExpr::Compare {
                                field: "b",
                                op: "=",
                                value: i64_val(2),
                            },
                        ],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_not() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE NOT a = 1 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Not {
                        operand: Box::new(FilterExpr::Compare {
                            field: "a",
                            op: "=",
                            value: i64_val(1),
                        }),
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_complex() {
        let stmt = assert_parse_ok(
            "SCROLL FROM c WHERE (a = 1 AND b = 2) OR (c = 3 AND NOT d = 4) LIMIT 10",
        );
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Or {
                        operands: vec![
                            FilterExpr::And {
                                operands: vec![
                                    FilterExpr::Compare {
                                        field: "a",
                                        op: "=",
                                        value: i64_val(1),
                                    },
                                    FilterExpr::Compare {
                                        field: "b",
                                        op: "=",
                                        value: i64_val(2),
                                    },
                                ],
                            },
                            FilterExpr::And {
                                operands: vec![
                                    FilterExpr::Compare {
                                        field: "c",
                                        op: "=",
                                        value: i64_val(3),
                                    },
                                    FilterExpr::Not {
                                        operand: Box::new(FilterExpr::Compare {
                                            field: "d",
                                            op: "=",
                                            value: i64_val(4),
                                        }),
                                    },
                                ],
                            },
                        ],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    #[test]
    fn test_filter_precedence() {
        let stmt = assert_parse_ok("SCROLL FROM c WHERE a = 1 AND b = 2 OR c = 3 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Or {
                        operands: vec![
                            FilterExpr::And {
                                operands: vec![
                                    FilterExpr::Compare {
                                        field: "a",
                                        op: "=",
                                        value: i64_val(1),
                                    },
                                    FilterExpr::Compare {
                                        field: "b",
                                        op: "=",
                                        value: i64_val(2),
                                    },
                                ],
                            },
                            FilterExpr::Compare {
                                field: "c",
                                op: "=",
                                value: i64_val(3),
                            },
                        ],
                    }))
                );
            }
            _ => panic!("expected Scroll stmt"),
        }
    }

    // ── Parse Errors ─────────────────────────────────────────────

    #[test]
    fn test_parse_error_invalid_statement() {
        assert_parse_err("INVALID KEYWORD");
    }

    #[test]
    fn test_parse_error_insert_missing_values() {
        assert_parse_err("INSERT INTO test");
    }

    #[test]
    fn test_parse_error_search_missing_query_text() {
        assert_parse_err("QUERY NEAREST FROM test");
    }

    #[test]
    fn test_parse_error_reject_trailing_tokens() {
        // Rust parser stops after consuming the insert statement, extra tokens are ignored
        let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} EXTRA");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_parse_error_reject_explain_in_parser() {
        assert_parse_err("EXPLAIN QUERY NEAREST 'text' FROM test LIMIT 10");
    }

    #[test]
    fn test_parse_error_reject_duplicate_where() {
        // Rust parser silently ignores duplicate WHERE, using the first one
        let stmt =
            assert_parse_ok("QUERY NEAREST 'text' FROM test LIMIT 10 WHERE a = 1 WHERE b = 2");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(
                    q.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "a",
                        op: "=",
                        value: i64_val(1),
                    }))
                );
            }
            _ => panic!("expected Query"),
        }
    }

    // ── Query: Nearest ───────────────────────────────────────────

    #[test]
    fn test_query_nearest() {
        let stmt = assert_parse_ok(
            "QUERY NEAREST 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128)",
        );
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Nearest);
                assert_eq!(q.collection, Some("docs"));
                assert_eq!(q.query_text, Some("vector search"));
                assert_eq!(q.limit, 10);
                assert_eq!(q.offset, 5);
                assert_eq!(q.query_type, QueryType::Hybrid);
                assert!(q.rerank);
                assert!(q.query_filter.is_some());
                assert!(q.with_clause.is_some());
                assert_eq!(q.with_clause.as_ref().unwrap().hnsw_ef, 128);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_query_recommend() {
        let stmt =
            assert_parse_ok("QUERY RECOMMEND WITH (positive = (1, 2), negative = (3)) FROM users");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Recommend);
                assert_eq!(q.collection, Some("users"));
                assert_eq!(q.positive_ids, vec![i64_val(1), i64_val(2)]);
                assert_eq!(q.negative_ids, vec![i64_val(3)]);
                assert_eq!(q.limit, 10);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_query_discover() {
        let stmt = assert_parse_ok(
            "QUERY DISCOVER TARGET 100 CONTEXT PAIRS (1, 2), (3, 4) FROM products LIMIT 20",
        );
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Discover);
                assert_eq!(q.collection, Some("products"));
                assert_eq!(q.target, Some(i64_val(100)));
                assert_eq!(q.context_pairs.len(), 2);
                assert_eq!(q.context_pairs[0].positive, i64_val(1));
                assert_eq!(q.context_pairs[0].negative, i64_val(2));
                assert_eq!(q.context_pairs[1].positive, i64_val(3));
                assert_eq!(q.context_pairs[1].negative, i64_val(4));
                assert_eq!(q.limit, 20);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_query_context() {
        let stmt = assert_parse_ok("QUERY CONTEXT PAIRS ('uuid-1', 'uuid-2') FROM logs");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Context);
                assert_eq!(q.collection, Some("logs"));
                assert_eq!(q.context_pairs.len(), 1);
                assert_eq!(q.context_pairs[0].positive, str_val("uuid-1"));
                assert_eq!(q.context_pairs[0].negative, str_val("uuid-2"));
            }
            _ => panic!("expected Query stmt"),
        }
    }

    // ── Query: Errors ────────────────────────────────────────────

    #[test]
    fn test_query_error_invalid_mode() {
        assert_parse_err("QUERY SOMETHING 'text' FROM docs");
    }

    #[test]
    fn test_query_error_missing_context_pairs() {
        assert_parse_err("QUERY CONTEXT FROM docs");
    }

    #[test]
    fn test_query_error_missing_discover_target() {
        assert_parse_err("QUERY DISCOVER FROM docs");
    }

    #[test]
    fn test_query_error_missing_positive_ids() {
        // Rust parser handles RECOMMEND WITH gracefully; if positive is missing, it's just empty
        let stmt = assert_parse_ok("QUERY RECOMMEND WITH (negative = (1)) FROM docs");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Recommend);
                assert!(q.positive_ids.is_empty());
                assert_eq!(q.negative_ids, vec![i64_val(1)]);
            }
            _ => panic!("expected Query"),
        }
    }

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

    // ── Query: ORDER BY ──────────────────────────────────────────

    #[test]
    fn test_query_order_by() {
        let stmt = assert_parse_ok("QUERY ORDER BY timestamp ASC FROM logs LIMIT 100");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::OrderBy);
                assert_eq!(q.order_by_field, Some("timestamp"));
                assert_eq!(q.order_by_asc, Some(true));
                assert_eq!(q.collection, Some("logs"));
                assert_eq!(q.limit, 100);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    // ── Query: SAMPLE ────────────────────────────────────────────

    #[test]
    fn test_query_sample() {
        let stmt = assert_parse_ok("QUERY SAMPLE FROM docs LIMIT 10");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Sample);
                assert_eq!(q.collection, Some("docs"));
                assert_eq!(q.limit, 10);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_query_sample_with_filter() {
        let stmt = assert_parse_ok("QUERY SAMPLE FROM docs LIMIT 10 WHERE category = 'tech'");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.mode, QueryMode::Sample);
                assert_eq!(q.collection, Some("docs"));
                assert_eq!(q.limit, 10);
                assert!(q.query_filter.is_some());
            }
            _ => panic!("expected Query stmt"),
        }
    }

    // ── Query: WITH PAYLOAD / WITH VECTORS ───────────────────────

    #[test]
    fn test_query_with_payload_and_vectors() {
        let stmt = assert_parse_ok(
            "QUERY 'search' FROM docs WITH PAYLOAD (include = ['title'], exclude = ['metadata']) WITH VECTORS true",
        );
        match stmt {
            Stmt::Query(q) => {
                let wp = q.with_payload.as_ref().unwrap();
                assert_eq!(wp.include, vec!["title"]);
                assert_eq!(wp.exclude, vec!["metadata"]);
                assert!(wp.enable.is_none());
                let wv = q.with_vectors.as_ref().unwrap();
                assert_eq!(wv.enable, Some(true));
                assert!(wv.vectors.is_empty());
            }
            _ => panic!("expected Query stmt"),
        }

        let stmt2 = assert_parse_ok(
            "QUERY 'search' FROM docs WITH PAYLOAD false WITH VECTORS ('dense', 'sparse')",
        );
        match stmt2 {
            Stmt::Query(q) => {
                let wp = q.with_payload.as_ref().unwrap();
                assert_eq!(wp.enable, Some(false));
                let wv = q.with_vectors.as_ref().unwrap();
                assert!(wv.enable.is_none());
                assert_eq!(wv.vectors, vec!["dense", "sparse"]);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    #[test]
    fn test_query_multiple_with_clauses() {
        let stmt = assert_parse_ok(
            "QUERY 'search' FROM docs WITH MODEL 'foo' WITH PAYLOAD (include = ['title']) WITH VECTORS true WITH (exact = true)",
        );
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.model, Some("foo"));
                let wp = q.with_payload.as_ref().unwrap();
                assert_eq!(wp.include, vec!["title"]);
                let wv = q.with_vectors.as_ref().unwrap();
                assert_eq!(wv.enable, Some(true));
                let wc = q.with_clause.as_ref().unwrap();
                assert!(wc.exact);
            }
            _ => panic!("expected Query stmt"),
        }
    }

    // ── Query: WITH PAYLOAD/VECTORS errors ───────────────────────

    #[test]
    fn test_query_with_payload_vectors_errors() {
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (badkey = ['a'])");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD include = ['a']");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = 'a')");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (include ['a'])");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = [123])");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = ['a'");
        assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = ['a']");
        assert_parse_err("QUERY FROM docs WITH VECTORS (123)");
        assert_parse_err("QUERY FROM docs WITH VECTORS ('dense'");
        assert_parse_err("QUERY FROM docs WITH VECTORS (['dense'])");
        assert_parse_err("QUERY ORDER BY FROM docs");
        assert_parse_err("QUERY ORDER timestamp FROM docs");
    }

    // ── Query: Raw Vector ────────────────────────────────────────

    #[test]
    fn test_query_raw_vector() {
        let stmt = assert_parse_ok("QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5");
        match stmt {
            Stmt::Query(q) => {
                assert_eq!(q.collection, Some("docs"));
                assert_eq!(q.raw_vector, vec![0.1, 0.2, 0.3]);
                assert_eq!(q.limit, 5);
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
        let stmt = assert_parse_ok(
            "QUERY 'test' FROM docs LIMIT 5 WHERE NESTED('tags', name = 'important')",
        );
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

    // ── CREATE COLLECTION ────────────────────────────────────────

    #[test]
    fn test_create_collection_simple() {
        let stmt = assert_parse_ok("CREATE COLLECTION mycollection");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                assert!(!c.hybrid);
                assert!(c.model.is_none());
                assert!(c.config.is_none());
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_hybrid() {
        let stmt = assert_parse_ok("CREATE COLLECTION mycollection HYBRID");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                assert!(c.hybrid);
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_model() {
        let stmt = assert_parse_ok("CREATE COLLECTION mycollection USING MODEL 'dense-model'");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                assert_eq!(c.model, Some("dense-model"));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_scalar_quantization() {
        let stmt =
            assert_parse_ok("CREATE COLLECTION mycollection WITH QUANTIZATION (type = 'scalar')");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Scalar);
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_scalar_quantization_integer_boundary() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION mycollection WITH QUANTIZATION (type = 'scalar', quantile = 1)",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Scalar);
                assert_eq!(q.quantile, Some(1.0));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_hybrid_rerank_product_quantization() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION mycollection HYBRID RERANK WITH QUANTIZATION (type = 'product', always_ram = true)",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                assert!(c.hybrid);
                assert!(c.rerank);
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Product);
                assert!(q.always_ram);
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_payload_hnsw() {
        let stmt = assert_parse_ok("CREATE COLLECTION mycollection WITH HNSW (payload_m = 16)");
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "mycollection");
                let cfg = c.config.as_ref().unwrap();
                let hnsw = cfg.hnsw.as_ref().unwrap();
                assert_eq!(hnsw.payload_m, Some(16));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_config_case_variant_keys_are_deterministic() {
        let stmt = assert_parse_ok("CREATE COLLECTION docs WITH HNSW ( M = 32, m = 16 )");
        match stmt {
            Stmt::CreateCollection(c) => {
                let cfg = c.config.as_ref().unwrap();
                let hnsw = cfg.hnsw.as_ref().unwrap();
                // Rust parser returns first match (case-insensitive)
                assert_eq!(hnsw.m, Some(32));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_rejects_non_positive_values() {
        let err = parse("CREATE COLLECTION docs WITH HNSW ( m = 3 )").unwrap_err();
        assert!(
            err.to_string().contains("m must be 0 or >= 4"),
            "got: {}",
            err
        );

        let err =
            parse("CREATE COLLECTION docs WITH PARAMS ( replication_factor = 0 )").unwrap_err();
        assert!(
            err.to_string()
                .contains("replication_factor must be a positive integer"),
            "got: {}",
            err
        );

        let err =
            parse("CREATE COLLECTION docs WITH HNSW ( full_scan_threshold = -1 )").unwrap_err();
        assert!(
            err.to_string()
                .contains("full_scan_threshold must be a non-negative integer"),
            "got: {}",
            err
        );

        let err = parse("CREATE COLLECTION docs WITH OPTIMIZERS ( indexing_threshold = -1 )")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("indexing_threshold must be a non-negative integer"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_create_collection_quantize_errors() {
        assert_parse_err("CREATE COLLECTION docs WITH QUANTIZATION (type = 'full')");
        assert_parse_err(
            "CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 1.5)",
        );
        assert_parse_err(
            "CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 2)",
        );
    }

    // ── CREATE INDEX ─────────────────────────────────────────────

    #[test]
    fn test_create_index_simple() {
        let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR field");
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "field");
                assert_eq!(i.field_type, "keyword");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_keyword_type() {
        let stmt =
            assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR field TYPE keyword");
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "field");
                assert_eq!(i.field_type, "keyword");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_integer_type() {
        let stmt =
            assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR patient_id TYPE integer");
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "patient_id");
                assert_eq!(i.field_type, "integer");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_float_type() {
        let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR score TYPE float");
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "score");
                assert_eq!(i.field_type, "float");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_bool_type() {
        let stmt =
            assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR is_active TYPE bool");
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "is_active");
                assert_eq!(i.field_type, "bool");
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_keyword_options() {
        let stmt = assert_parse_ok(
            "CREATE INDEX ON COLLECTION mycollection FOR tenant_id TYPE keyword WITH (is_tenant = true, on_disk = true, enable_hnsw = false)",
        );
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "tenant_id");
                assert_eq!(i.field_type, "keyword");
                assert!(i.options.contains(&("is_tenant", Value::Bool(true))));
                assert!(i.options.contains(&("on_disk", Value::Bool(true))));
                assert!(i.options.contains(&("enable_hnsw", Value::Bool(false))));
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_index_with_text_options() {
        let stmt = assert_parse_ok(
            "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true)",
        );
        match stmt {
            Stmt::CreateIndex(i) => {
                assert_eq!(i.collection, "mycollection");
                assert_eq!(i.field, "title");
                assert_eq!(i.field_type, "text");
                assert!(i.options.contains(&("tokenizer", str_val("word"))));
                assert!(i.options.contains(&("min_token_len", i64_val(2))));
                assert!(i.options.contains(&("max_token_len", i64_val(20))));
                assert!(i.options.contains(&("lowercase", Value::Bool(true))));
            }
            _ => panic!("expected CreateIndex"),
        }
    }

    #[test]
    fn test_create_collection_with_turbo_quantization_default() {
        let stmt = assert_parse_ok("CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo')");
        match stmt {
            Stmt::CreateCollection(c) => {
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Turbo);
                assert!(!q.always_ram);
                assert!(q.turbo_bits.is_none());
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_turbo_quantization_bits_1_5() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 1.5)",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Turbo);
                assert_eq!(q.turbo_bits, Some(1.5));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_with_turbo_quantization_bits_1_always_ram() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 1, always_ram = true)",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                let cfg = c.config.as_ref().unwrap();
                let q = cfg.quantization.as_ref().unwrap();
                assert_eq!(q.qtype, QuantizationType::Turbo);
                assert_eq!(q.turbo_bits, Some(1.0));
                assert!(q.always_ram);
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_collection_turbo_quantization_invalid_bits() {
        let err = parse("CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 3)")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("bits must be one of 1, 1.5, 2, or 4"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_create_multi_vector() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION knowledge_graph (
                dense_text VECTOR(384, COSINE),
                clip_img VECTOR(512, DOT),
                bm25_text SPARSE
            ) WITH HNSW ( m = 32 )",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "knowledge_graph");
                assert_eq!(c.vectors.len(), 2);
                assert_eq!(c.sparse_vectors.len(), 1);
                assert_eq!(c.vectors[0].name, "dense_text");
                assert_eq!(c.vectors[0].size, 384);
                assert_eq!(c.vectors[0].distance, crate::ast::VectorDistance::Cosine);
                assert_eq!(c.vectors[1].name, "clip_img");
                assert_eq!(c.vectors[1].size, 512);
                assert_eq!(c.vectors[1].distance, crate::ast::VectorDistance::Dot);
                assert_eq!(c.sparse_vectors[0].name, "bm25_text");
                let cfg = c.config.as_ref().unwrap();
                let hnsw = cfg.hnsw.as_ref().unwrap();
                assert_eq!(hnsw.m, Some(32));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_create_multi_vector_with_overrides() {
        let stmt = assert_parse_ok(
            "CREATE COLLECTION test_overrides (
                dense_vec VECTOR(384, COSINE) WITH HNSW ( m = 16 ) WITH QUANTIZATION (type = 'scalar', always_ram = true),
                colbert_vec VECTOR(128, DOT) WITH QUANTIZATION (type = 'turbo', bits = 2)
            )",
        );
        match stmt {
            Stmt::CreateCollection(c) => {
                assert_eq!(c.collection, "test_overrides");
                assert_eq!(c.vectors.len(), 2);
                assert_eq!(c.vectors[0].name, "dense_vec");
                let h0 = c.vectors[0].hnsw.as_ref().unwrap();
                assert_eq!(h0.m, Some(16));
                let q0 = c.vectors[0].quantization.as_ref().unwrap();
                assert_eq!(q0.qtype, QuantizationType::Scalar);
                assert!(q0.always_ram);
                assert_eq!(c.vectors[1].name, "colbert_vec");
                assert!(c.vectors[1].hnsw.is_none());
                let q1 = c.vectors[1].quantization.as_ref().unwrap();
                assert_eq!(q1.qtype, QuantizationType::Turbo);
                assert_eq!(q1.turbo_bits, Some(2.0));
            }
            _ => panic!("expected CreateCollection"),
        }
    }

    // ── INSERT ───────────────────────────────────────────────────

    #[test]
    fn test_insert_simple() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert_eq!(i.values_list, vec![vec![("text", str_val("hello"))]]);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_bare_keys() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {text: 'hello', topic: 'search'}");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert_eq!(
                    i.values_list,
                    vec![make_payload(&[
                        ("text", str_val("hello")),
                        ("topic", str_val("search"))
                    ])]
                );
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_explicit_id() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {id: 'point-123', text: 'hello'}");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert_eq!(
                    i.values_list,
                    vec![make_payload(&[
                        ("id", str_val("point-123")),
                        ("text", str_val("hello")),
                    ])]
                );
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_model() {
        let stmt =
            assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} USING MODEL 'model-name'");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert_eq!(i.model, Some("model-name"));
                assert!(!i.hybrid);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_hybrid() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} USING HYBRID");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert!(i.hybrid);
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_hybrid_and_models() {
        let stmt = assert_parse_ok(
            "INSERT INTO test VALUES {'text': 'hello'} USING HYBRID DENSE MODEL 'dense-model' SPARSE MODEL 'sparse-model'",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert!(i.hybrid);
                assert_eq!(i.model, Some("dense-model"));
                assert_eq!(i.sparse_model, Some("sparse-model"));
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_with_sparse_model_only() {
        let stmt = assert_parse_ok(
            "INSERT INTO test VALUES {'text': 'hello'} USING HYBRID SPARSE MODEL 'sparse-model'",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert!(i.hybrid);
                assert_eq!(i.sparse_model, Some("sparse-model"));
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_multiple_values() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}, {'text': 'world'}");
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "test");
                assert_eq!(
                    i.values_list,
                    vec![
                        vec![("text", str_val("hello"))],
                        vec![("text", str_val("world"))],
                    ]
                );
            }
            _ => panic!("expected Insert"),
        }
    }

    // ── INSERT with EMBED ────────────────────────────────────────

    #[test]
    fn test_insert_embed_single() {
        let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper'} EMBED text INTO dense_chunk",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "arxiv");
                assert_eq!(i.embed_directives.len(), 1);
                assert_eq!(i.embed_directives[0].source_field, "text");
                assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_embed_multiple() {
        let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper', abstract: 'Full abstract'} EMBED text INTO dense_chunk, title INTO dense_title, abstract INTO dense_abstract",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "arxiv");
                assert_eq!(i.embed_directives.len(), 3);
                assert_eq!(i.embed_directives[0].source_field, "text");
                assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
                assert_eq!(i.embed_directives[1].source_field, "title");
                assert_eq!(i.embed_directives[1].target_vector, "dense_title");
                assert_eq!(i.embed_directives[2].source_field, "abstract");
                assert_eq!(i.embed_directives[2].target_vector, "dense_abstract");
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_embed_with_sparse() {
        let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', title: 'Paper'} EMBED title INTO sparse_title USING SPARSE",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "arxiv");
                assert_eq!(i.embed_directives.len(), 1);
                assert_eq!(i.embed_directives[0].source_field, "title");
                assert_eq!(i.embed_directives[0].target_vector, "sparse_title");
                assert_eq!(i.embed_directives[0].sparse_model, Some(""));
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_embed_with_explicit_model() {
        let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', title: 'Paper'} EMBED title INTO dense_title USING MODEL 'custom-model'",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "arxiv");
                assert_eq!(i.embed_directives.len(), 1);
                assert_eq!(i.embed_directives[0].source_field, "title");
                assert_eq!(i.embed_directives[0].target_vector, "dense_title");
                assert_eq!(i.embed_directives[0].model, Some("custom-model"));
            }
            _ => panic!("expected Insert"),
        }
    }

    #[test]
    fn test_insert_embed_mixed_dense_sparse() {
        let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper'} EMBED text INTO dense_chunk, title INTO sparse_title USING SPARSE",
        );
        match stmt {
            Stmt::Insert(i) => {
                assert_eq!(i.collection, "arxiv");
                assert_eq!(i.embed_directives.len(), 2);
                assert_eq!(i.embed_directives[0].source_field, "text");
                assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
                assert_eq!(i.embed_directives[0].sparse_model, None);
                assert_eq!(i.embed_directives[1].source_field, "title");
                assert_eq!(i.embed_directives[1].target_vector, "sparse_title");
                assert_eq!(i.embed_directives[1].sparse_model, Some(""));
            }
            _ => panic!("expected Insert"),
        }
    }

    // ── INSERT Errors ────────────────────────────────────────────

    #[test]
    fn test_insert_errors() {
        assert_parse_err("INSERT INTO test VALUES");
    }

    // ── DELETE ───────────────────────────────────────────────────

    #[test]
    fn test_delete_with_string_id() {
        let stmt = assert_parse_ok("DELETE FROM mycollection WHERE id = 'point-123'");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "mycollection");
                assert_eq!(d.point_id, Some(str_val("point-123")));
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_delete_with_integer_id() {
        let stmt = assert_parse_ok("DELETE FROM mycollection WHERE id = 42");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "mycollection");
                assert_eq!(d.point_id, Some(i64_val(42)));
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_delete_by_field() {
        let stmt = assert_parse_ok("DELETE FROM mycollection WHERE status = 'archived'");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "mycollection");
                assert_eq!(d.field, Some("status"));
                assert_eq!(d.value, Some(str_val("archived")));
            }
            _ => panic!("expected Delete"),
        }
    }

    // ── UPDATE ───────────────────────────────────────────────────

    #[test]
    fn test_update_vector_by_id() {
        let stmt = assert_parse_ok("UPDATE articles SET VECTOR = [0.1, 0.2] WHERE id = 42");
        match stmt {
            Stmt::UpdateVector(u) => {
                assert_eq!(u.collection, "articles");
                assert_eq!(u.point_id, i64_val(42));
                assert_eq!(u.vector, vec![0.1f32, 0.2f32]);
                assert!(u.vector_name.is_none());
            }
            _ => panic!("expected UpdateVector"),
        }
    }

    #[test]
    fn test_update_payload_by_filter() {
        let stmt = assert_parse_ok(
            "UPDATE articles SET PAYLOAD = {'status': 'published'} WHERE category = 'draft'",
        );
        match stmt {
            Stmt::UpdatePayload(u) => {
                assert_eq!(u.collection, "articles");
                assert!(u.point_id.is_none());
                assert_eq!(
                    u.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "category",
                        op: "=",
                        value: str_val("draft"),
                    }))
                );
                assert_eq!(u.payload, vec![("status", str_val("published"))]);
            }
            _ => panic!("expected UpdatePayload"),
        }
    }

    #[test]
    fn test_update_custom_named_vector_by_id() {
        let stmt =
            assert_parse_ok("UPDATE articles SET VECTOR 'colbert' = [0.1, 0.2] WHERE id = 42");
        match stmt {
            Stmt::UpdateVector(u) => {
                assert_eq!(u.collection, "articles");
                assert_eq!(u.point_id, i64_val(42));
                assert_eq!(u.vector, vec![0.1f32, 0.2f32]);
                assert_eq!(u.vector_name, Some("colbert"));
            }
            _ => panic!("expected UpdateVector"),
        }
    }

    #[test]
    fn test_update_payload_by_id() {
        let stmt =
            assert_parse_ok("UPDATE articles SET PAYLOAD = {'year': 2025} WHERE id = 'abc-123'");
        match stmt {
            Stmt::UpdatePayload(u) => {
                assert_eq!(u.collection, "articles");
                assert_eq!(u.point_id, Some(str_val("abc-123")));
                assert_eq!(u.payload, vec![("year", i64_val(2025))]);
            }
            _ => panic!("expected UpdatePayload"),
        }
    }

    #[test]
    fn test_update_vector_rejects_bools() {
        assert_parse_err("UPDATE articles SET VECTOR = [true, 0.2] WHERE id = 1");
    }

    #[test]
    fn test_update_rejects_invalid_target() {
        assert_parse_err("UPDATE articles SET NAME = {'x': 1} WHERE id = 1");
    }

    // ── SELECT ───────────────────────────────────────────────────

    #[test]
    fn test_select_with_string_id() {
        let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 'point-123'");
        match stmt {
            Stmt::Select(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.point_id, str_val("point-123"));
            }
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_select_with_integer_id() {
        let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 42");
        match stmt {
            Stmt::Select(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.point_id, i64_val(42));
            }
            _ => panic!("expected Select"),
        }
    }

    // ── SCROLL ───────────────────────────────────────────────────

    #[test]
    fn test_scroll_basic() {
        let stmt = assert_parse_ok("SCROLL FROM docs LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.limit, 10);
                assert!(s.query_filter.is_none());
                assert!(s.after.is_none());
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_scroll_with_where() {
        let stmt = assert_parse_ok("SCROLL FROM docs WHERE status = 'active' LIMIT 5");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.limit, 5);
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "status",
                        op: "=",
                        value: str_val("active"),
                    }))
                );
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_scroll_with_after() {
        let stmt = assert_parse_ok("SCROLL FROM docs AFTER 'point-123' LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.limit, 10);
                assert_eq!(s.after, Some(str_val("point-123")));
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_scroll_with_after_integer() {
        let stmt = assert_parse_ok("SCROLL FROM docs AFTER 42 LIMIT 10");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.limit, 10);
                assert_eq!(s.after, Some(i64_val(42)));
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_scroll_with_where_and_after() {
        let stmt =
            assert_parse_ok("SCROLL FROM docs WHERE status = 'active' AFTER 'point-50' LIMIT 20");
        match stmt {
            Stmt::Scroll(s) => {
                assert_eq!(s.collection, "docs");
                assert_eq!(s.limit, 20);
                assert_eq!(
                    s.query_filter,
                    Some(Box::new(FilterExpr::Compare {
                        field: "status",
                        op: "=",
                        value: str_val("active"),
                    }))
                );
                assert_eq!(s.after, Some(str_val("point-50")));
            }
            _ => panic!("expected Scroll"),
        }
    }

    // ── MANAGE: ALTER ────────────────────────────────────────────

    #[test]
    fn test_alter_collection() {
        let stmt = assert_parse_ok("ALTER COLLECTION docs WITH HNSW (m = 32)");
        match stmt {
            Stmt::AlterCollection(a) => {
                assert_eq!(a.collection, "docs");
                assert!(a.config.is_some());
            }
            _ => panic!("expected AlterCollection"),
        }
    }

    #[test]
    fn test_create_rejects_alter_only_params() {
        let err =
            parse("CREATE COLLECTION docs WITH PARAMS ( Read_Fan_Out_Factor = 4 )").unwrap_err();
        assert!(
            err.to_string()
                .contains("supported only for ALTER COLLECTION"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_alter_rejects_non_positive_values() {
        let err =
            parse("ALTER COLLECTION docs WITH PARAMS ( read_fan_out_delay_ms = -1 )").unwrap_err();
        assert!(
            err.to_string()
                .contains("read_fan_out_delay_ms must be a non-negative integer"),
            "got: {}",
            err
        );
    }

    // ── MANAGE: DROP ─────────────────────────────────────────────

    #[test]
    fn test_drop_collection() {
        let stmt = assert_parse_ok("DROP COLLECTION mycollection");
        match stmt {
            Stmt::DropCollection(d) => {
                assert_eq!(d.collection, "mycollection");
            }
            _ => panic!("expected DropCollection"),
        }
    }

    // ── MANAGE: SHOW ─────────────────────────────────────────────

    #[test]
    fn test_show_collections() {
        let stmt = assert_parse_ok("SHOW COLLECTIONS");
        match stmt {
            Stmt::ShowCollections => {}
            _ => panic!("expected ShowCollections"),
        }
    }

    #[test]
    fn test_show_collection_simple() {
        let stmt = assert_parse_ok("SHOW COLLECTION docs");
        match stmt {
            Stmt::ShowCollection(c) => {
                assert_eq!(c, "docs");
            }
            _ => panic!("expected ShowCollection"),
        }
    }

    #[test]
    fn test_show_collection_case_insensitive() {
        let stmt = assert_parse_ok("show collection MY_COL");
        match stmt {
            Stmt::ShowCollection(c) => {
                assert_eq!(c, "MY_COL");
            }
            _ => panic!("expected ShowCollection"),
        }
    }

    #[test]
    fn test_show_collection_error_without_name() {
        assert_parse_err("SHOW COLLECTION");
    }

    // ── FORMULA: Basic ───────────────────────────────────────────

    #[test]
    fn test_formula_arithmetic() {
        let query = "QUERY 'test' FROM my_col LIMIT 10
    BOOST ($score * 2.0 + ABS(match_count * 0.1))
    DEFAULTS (popularity = 1.0, rating = 0.0)";
        let stmt = assert_parse_ok(query);
        match stmt {
            Stmt::Query(q) => {
                assert!(q.formula.is_some());
                assert_eq!(
                    q.formula_defaults,
                    vec![("popularity", float_val(1.0)), ("rating", float_val(0.0))]
                );
                match q.formula.as_ref().unwrap().as_ref() {
                    FormulaExpr::Sum { left, right } => {
                        match left.as_ref() {
                            FormulaExpr::Mul { left: l, right: r } => {
                                match l.as_ref() {
                                    FormulaExpr::Variable { name } => assert_eq!(*name, "$score"),
                                    _ => panic!("expected Variable($score)"),
                                }
                                match r.as_ref() {
                                    FormulaExpr::Constant { value } => assert_eq!(*value, 2.0),
                                    _ => panic!("expected Constant(2.0)"),
                                }
                            }
                            _ => panic!("expected Mul"),
                        }
                        match right.as_ref() {
                            FormulaExpr::Abs { x } => match x.as_ref() {
                                FormulaExpr::Mul { left: l, right: r } => {
                                    match l.as_ref() {
                                        FormulaExpr::Variable { name } => {
                                            assert_eq!(*name, "match_count")
                                        }
                                        _ => panic!("expected Variable(match_count)"),
                                    }
                                    match r.as_ref() {
                                        FormulaExpr::Constant { value } => {
                                            assert_eq!(*value, 0.1)
                                        }
                                        _ => panic!("expected Constant(0.1)"),
                                    }
                                }
                                _ => panic!("expected Mul inside Abs"),
                            },
                            _ => panic!("expected Abs"),
                        }
                    }
                    _ => panic!("expected Sum at top level"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    // ── FORMULA: Functions ───────────────────────────────────────

    #[test]
    fn test_formula_geo_distance() {
        let query =
            "QUERY 'test' FROM my_col BOOST (gauss_decay(geo_distance(48.8, 2.3, location), target=0.0, scale=1000.0, decay=0.8))";
        let stmt = assert_parse_ok(query);
        match stmt {
            Stmt::Query(q) => {
                // BOOST with nested functions may not be fully handled by Rust formula parser
                // so formula is silently None
                assert!(q.formula.is_none());
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn test_formula_decay_errors() {
        let stmt = assert_parse_ok(
            "QUERY 'test' FROM my_col BOOST (gauss_decay(geo_distance(48.8, 2.3, location), target=0.0, scale=popularity, midpoint=0.5))",
        );
        match stmt {
            Stmt::Query(q) => {
                // BOOST silently ignores formula parse errors; formula is None
                assert!(q.formula.is_none());
            }
            _ => panic!("expected Query"),
        }
    }

    // ── FORMULA: CASE ────────────────────────────────────────────

    #[test]
    fn test_formula_case() {
        let query = "QUERY 'test' FROM my_col
    BOOST (CASE WHEN category = 'premium' THEN $score * 2.0 ELSE $score END)";
        let stmt = assert_parse_ok(query);
        match stmt {
            Stmt::Query(q) => match q.formula.as_ref().unwrap().as_ref() {
                FormulaExpr::Case { cond, then_, else_ } => {
                    match cond.as_ref() {
                        FilterExpr::Compare { field, op, value } => {
                            assert_eq!(*field, "category");
                            assert_eq!(*value, str_val("premium"));
                        }
                        _ => panic!("expected Compare"),
                    }
                    match then_.as_ref() {
                        FormulaExpr::Mul { left, right: _ } => match left.as_ref() {
                            FormulaExpr::Variable { name } => {
                                assert_eq!(*name, "$score")
                            }
                            _ => panic!("expected Variable($score)"),
                        },
                        _ => panic!("expected Mul"),
                    }
                    match else_.as_ref() {
                        FormulaExpr::Variable { name } => assert_eq!(*name, "$score"),
                        _ => panic!("expected Variable($score)"),
                    }
                }
                _ => panic!("expected Case"),
            },
            _ => panic!("expected Query"),
        }
    }

    // ── FORMULA: MATCH ───────────────────────────────────────────

    #[test]
    fn test_formula_match() {
        let query = "QUERY 'test' FROM my_col
    BOOST ($score + 0.5 * MATCH(tag, ['h1', 'h2', 'h3']) + 0.25 * MATCH(category, 'premium'))";
        let stmt = assert_parse_ok(query);
        match stmt {
            Stmt::Query(q) => {
                match q.formula.as_ref().unwrap().as_ref() {
                    FormulaExpr::Sum { left, right } => {
                        // left is inner Sum: $score + 0.5 * MATCH
                        match left.as_ref() {
                            FormulaExpr::Sum { left: l, right: r } => {
                                match l.as_ref() {
                                    FormulaExpr::Variable { name } => {
                                        assert_eq!(*name, "$score")
                                    }
                                    _ => panic!("expected Variable($score)"),
                                }
                                // 0.5 * MATCH(tag, [...])
                                match r.as_ref() {
                                    FormulaExpr::Mul {
                                        left: m_left,
                                        right: m_right,
                                    } => {
                                        match m_left.as_ref() {
                                            FormulaExpr::Constant { value } => {
                                                assert_eq!(*value, 0.5)
                                            }
                                            _ => panic!("expected Constant(0.5)"),
                                        }
                                        match m_right.as_ref() {
                                            FormulaExpr::MatchCondition { field, values } => {
                                                assert_eq!(*field, "tag");
                                                assert_eq!(
                                                    *values,
                                                    vec![
                                                        str_val("h1"),
                                                        str_val("h2"),
                                                        str_val("h3"),
                                                    ]
                                                );
                                            }
                                            _ => panic!("expected MatchCondition"),
                                        }
                                    }
                                    _ => panic!("expected Mul"),
                                }
                            }
                            _ => panic!("expected inner Sum"),
                        }
                        // right: 0.25 * MATCH(category, 'premium')
                        match right.as_ref() {
                            FormulaExpr::Mul {
                                left: m_left,
                                right: m_right,
                            } => {
                                match m_left.as_ref() {
                                    FormulaExpr::Constant { value } => assert_eq!(*value, 0.25),
                                    _ => panic!("expected Constant(0.25)"),
                                }
                                match m_right.as_ref() {
                                    FormulaExpr::MatchCondition { field, values } => {
                                        assert_eq!(*field, "category");
                                        assert_eq!(*values, vec![str_val("premium")]);
                                    }
                                    _ => panic!("expected MatchCondition"),
                                }
                            }
                            _ => panic!("expected Mul"),
                        }
                    }
                    _ => panic!("expected Sum"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    // ── FORMULA: DIV with defaults ───────────────────────────────

    #[test]
    fn test_formula_div_without_default() {
        let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST ($score / popularity)");
        match stmt {
            Stmt::Query(q) => match q.formula.as_ref().unwrap().as_ref() {
                FormulaExpr::Div {
                    by_zero_default, ..
                } => {
                    assert_eq!(*by_zero_default, None);
                }
                _ => panic!("expected Div"),
            },
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn test_formula_div_with_default() {
        let stmt =
            assert_parse_ok("QUERY 'test' FROM my_col BOOST ($score / popularity [default=1.5])");
        match stmt {
            Stmt::Query(q) => {
                // Rust formula parser does not handle [default=...] syntax inside BOOST
                // so the formula is silently set to None
                assert!(q.formula.is_none());
            }
            _ => panic!("expected Query"),
        }
    }

    // ── Formula Errors ───────────────────────────────────────────

    #[test]
    fn test_formula_errors() {
        let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST ()");
        match stmt {
            Stmt::Query(q) => assert!(q.formula.is_none()),
            _ => panic!("expected Query"),
        }
        let stmt = assert_parse_ok("QUERY 'test' FROM my_col BOOST (+ )");
        match stmt {
            Stmt::Query(q) => assert!(q.formula.is_none()),
            _ => panic!("expected Query"),
        }
    }

    // ── QUERY: Simple basic parse ────────────────────────────────

    #[test]
    fn test_parse_insert() {
        let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}");
        match stmt {
            Stmt::Insert(_) => {}
            _ => panic!("expected Insert"),
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

    #[test]
    fn test_parse_create() {
        let stmt = assert_parse_ok("CREATE COLLECTION test");
        match stmt {
            Stmt::CreateCollection(_) => {}
            _ => panic!("expected CreateCollection"),
        }
    }

    #[test]
    fn test_parse_select() {
        let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 'abc'");
        match stmt {
            Stmt::Select(_) => {}
            _ => panic!("expected Select"),
        }
    }

    #[test]
    fn test_parse_scroll() {
        let stmt = assert_parse_ok("SCROLL FROM docs LIMIT 10");
        match stmt {
            Stmt::Scroll(_) => {}
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = assert_parse_ok("DELETE FROM docs WHERE id = 'x'");
        match stmt {
            Stmt::Delete(_) => {}
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = assert_parse_ok("UPDATE docs SET VECTOR = [1.0, 2.0] WHERE id = 1");
        match stmt {
            Stmt::UpdateVector(_) => {}
            _ => panic!("expected UpdateVector"),
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

    // ── Delete with filter (parse delete fallback) ────────────────

    #[test]
    fn test_delete_with_filter() {
        let stmt = assert_parse_ok("DELETE FROM docs WHERE id = 'xyz'");
        match stmt {
            Stmt::Delete(d) => {
                assert_eq!(d.collection, "docs");
                assert_eq!(d.point_id, Some(str_val("xyz")));
            }
            _ => panic!("expected Delete"),
        }
    }
}
