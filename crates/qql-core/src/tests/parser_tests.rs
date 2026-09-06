use crate::ast::{
    ComparisonOp, EmbeddingSpec, FilterExpr, FormulaExpr, FusionMethod, QueryCollection, QueryExpr,
    QueryInput, Stmt, Value,
};
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
fn sql_style_doubled_quote_is_decoded() {
    let stmt = Parser::parse("QUERY 'St. Peter''s Church' FROM docs LIMIT 1;").unwrap();
    let Stmt::Query(query) = stmt else {
        panic!("expected QUERY");
    };
    let QueryExpr::Nearest {
        input: QueryInput::Text { text, .. },
        ..
    } = query.expression
    else {
        panic!("expected nearest text query");
    };
    assert_eq!(text, "St. Peter's Church");
}

#[test]
fn sparse_upsert_embedding_is_explicit() {
    let stmt =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING SPARSE VECTOR sparse")
            .unwrap();
    let crate::ast::Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert");
    };
    assert!(matches!(
        upsert.embedding,
        Some(crate::ast::EmbeddingSpec::Sparse { .. })
    ));
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
    let s = Parser::parse("QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;")
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Vector(_), using: Some(ref u), ..
    } if u.name == "dense"));
    assert_eq!(q.page.limit, Some(5));
}

#[test]
fn nearest_point() {
    let s = Parser::parse("QUERY NEAREST POINT 42 FROM docs USING dense;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Point(crate::ast::PointId::Number(42)), using: Some(ref u), ..
    } if u.name == "dense"));
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
    )
    .unwrap();
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
    assert!(
        matches!(q.expression, QueryExpr::Fusion { method: FusionMethod::Rrf, ref prefetch } if prefetch.len() == 2)
    );
}

#[test]
fn fusion_dbsf() {
    let s = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY FUSION DBSF FROM docs PREFETCH (d) LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Fusion {
            method: FusionMethod::Dbsf,
            ..
        }
    ));
}

#[test]
fn formula_query() {
    let s = Parser::parse("QUERY FORMULA $score + 1 DEFAULTS (missing = 0) FROM docs;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Formula { .. }));
}

#[test]
fn formula_query_div_default() {
    let res = Parser::parse(
        "QUERY FORMULA ($score / views [DEFAULT = 1.0]) * 10 DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
    );
    assert!(res.is_ok(), "failed: {:?}", res.err());
}

#[test]
fn formula_max_min_acosh_functions() {
    let s = Parser::parse(
        "QUERY FORMULA MAX($score * 2.0, MIN($score, bonus)) + ACOSH(rank) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    let QueryExpr::Formula { expression, .. } = &q.expression else {
        panic!()
    };
    // Outer sum: MAX(...) + ACOSH(...)
    let FormulaExpr::Sum { left, right } = expression.as_ref() else {
        panic!()
    };
    let FormulaExpr::Max { args } = left.as_ref() else {
        panic!("expected MAX, got {left:?}")
    };
    assert_eq!(args.len(), 2, "MAX folds both operands");
    assert!(matches!(args[0], FormulaExpr::Mul { .. }));
    let FormulaExpr::Min { args } = &args[1] else {
        panic!("expected nested MIN, got {:?}", args[1])
    };
    assert_eq!(args.len(), 2);
    let FormulaExpr::Acosh { x } = right.as_ref() else {
        panic!("expected ACOSH, got {right:?}")
    };
    assert!(matches!(x.as_ref(), FormulaExpr::Variable { name } if name == "rank"));
}

#[test]
fn formula_functions_are_case_insensitive() {
    let s =
        Parser::parse("QUERY FORMULA max(1.0, min(2.0, 3.0)) DEFAULTS (score = 0.0) FROM docs;")
            .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    let QueryExpr::Formula { expression, .. } = &q.expression else {
        panic!()
    };
    assert!(matches!(expression.as_ref(), FormulaExpr::Max { args } if args.len() == 2));
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
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Nearest {
        input: QueryInput::Text { ref text, .. }, mmr: Some(_), ..
    } if text == "diverse"));
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
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Hybrid {
        ref text, fusion: FusionMethod::Rrf, ..
    } if text == "search"));
}

#[test]
fn using_hybrid_shorthand_expands_to_hybrid() {
    // Tail form: QUERY TEXT … USING HYBRID … → same AST as QUERY HYBRID TEXT …
    let s = Parser::parse(
        "QUERY TEXT 'search' FROM docs USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT 10;",
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Hybrid {
            ref text,
            dense_vector: Some(ref d),
            sparse_vector: Some(ref sp),
            fusion: FusionMethod::Rrf,
            model: None,
        } if text == "search" && d == "dense" && sp == "sparse"
    ));
}

#[test]
fn using_hybrid_defaults_fusion_rrf_and_omitted_names() {
    let s = Parser::parse("QUERY 'search' FROM docs USING HYBRID LIMIT 10;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Hybrid {
            ref text,
            dense_vector: None,
            sparse_vector: None,
            fusion: FusionMethod::Rrf,
            model: None,
        } if text == "search"
    ));
}

#[test]
fn using_hybrid_preserves_model_and_dbsf() {
    let s = Parser::parse(
        "QUERY TEXT 'q' MODEL 'nomic' FROM docs USING HYBRID DENSE d SPARSE s FUSION DBSF LIMIT 5;",
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Hybrid {
            ref text,
            model: Some(ref m),
            dense_vector: Some(ref d),
            sparse_vector: Some(ref sp),
            fusion: FusionMethod::Dbsf,
        } if text == "q" && m == "nomic" && d == "d" && sp == "s"
    ));
}

#[test]
fn using_hybrid_rejects_non_text_nearest() {
    assert!(Parser::parse("QUERY VECTOR [0.1, 0.2] FROM docs USING HYBRID LIMIT 10;").is_err());
    assert!(Parser::parse("QUERY IMAGE '/tmp/a.png' FROM docs USING HYBRID LIMIT 10;").is_err());
    assert!(
        Parser::parse(
            "QUERY MMR TEXT 'x' DIVERSITY 0.5 CANDIDATES 20 FROM docs USING HYBRID LIMIT 10;"
        )
        .is_err()
    );
    // Front-form already Hybrid — USING HYBRID is redundant/invalid.
    assert!(Parser::parse("QUERY HYBRID TEXT 'x' FROM docs USING HYBRID LIMIT 10;").is_err());
}

#[test]
fn rerank_query() {
    let s = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense LIMIT 100) QUERY RERANK TEXT 'x' MODEL 'reranker' FROM docs USING colbert PREFETCH (c) LIMIT 10;",
    ).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(q.expression, QueryExpr::Rerank {
        ref model, ref using, ..
    } if model == "reranker" && using.as_ref().is_some_and(|target| target.name == "colbert")));
}

#[test]
fn using_can_declare_an_arbitrary_sparse_vector() {
    let s = Parser::parse("QUERY TEXT 'search' FROM docs USING lexical_v2 AS SPARSE LIMIT 10;")
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Nearest {
            using: Some(crate::ast::VectorTarget {
                ref name,
                kind: Some(crate::ast::VectorKind::Sparse),
                multi: false,
            }),
            ..
        } if name == "lexical_v2"
    ));
}

#[test]
fn max_selectivity_requires_acorn() {
    assert!(Parser::parse("QUERY 'x' FROM docs PARAMS (max_selectivity = 0.5) LIMIT 1;").is_err());
    let ok =
        Parser::parse("QUERY 'x' FROM docs PARAMS (acorn = true, max_selectivity = 0.5) LIMIT 1;");
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn params_timeout_and_consistency() {
    use crate::ast::ReadConsistency;
    let s =
        Parser::parse("QUERY 'x' FROM docs PARAMS (timeout = 30, consistency = majority) LIMIT 5;")
            .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    let p = q.params.as_ref().unwrap();
    assert_eq!(p.timeout, Some(30));
    assert_eq!(p.consistency, Some(ReadConsistency::Majority));

    let s = Parser::parse("QUERY 'x' FROM docs PARAMS (consistency = 2) LIMIT 5;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert_eq!(
        q.params.as_ref().unwrap().consistency,
        Some(ReadConsistency::Factor(2))
    );
}

#[test]
fn params_idf_global_and_corpus() {
    let s = Parser::parse("QUERY 'x' FROM docs PARAMS (idf = 'global') LIMIT 5;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    let idf = q.params.as_ref().unwrap().idf.as_ref().unwrap();
    assert!(idf.corpus.is_none(), "global scope must carry no corpus");

    let s = Parser::parse("QUERY 'x' FROM docs PARAMS (idf = WHERE status = 'active') LIMIT 5;")
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    let idf = q.params.as_ref().unwrap().idf.as_ref().unwrap();
    match idf.corpus.as_ref().expect("corpus filter") {
        FilterExpr::Compare {
            field,
            op,
            value: Value::Str(value),
        } => {
            assert_eq!(field, "status");
            assert_eq!(*op, ComparisonOp::Eq);
            assert_eq!(value, "active");
        }
        other => panic!("expected compare corpus, got {other:?}"),
    }

    let tenant = Parser::parse(
        "QUERY 'x' FROM docs PARAMS (idf = WHERE tenant_id = 'acme' AND status = 'active') LIMIT 5;",
    )
    .unwrap();
    let Stmt::Query(q) = tenant else { panic!() };
    assert!(matches!(
        q.params
            .as_ref()
            .unwrap()
            .idf
            .as_ref()
            .unwrap()
            .corpus
            .as_ref()
            .unwrap(),
        FilterExpr::And { operands } if operands.len() == 2
    ));

    // Bare keyword global, and formatter round-trip of WHERE corpora.
    let s = Parser::parse("QUERY 'x' FROM docs PARAMS (idf = global) LIMIT 5;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(
        q.params
            .as_ref()
            .unwrap()
            .idf
            .as_ref()
            .unwrap()
            .corpus
            .is_none()
    );

    let formatted = crate::fmt::format_stmt(
        &Parser::parse("QUERY 'x' FROM docs PARAMS (idf = 'global') LIMIT 5;").unwrap(),
    );
    assert!(formatted.contains("idf = 'global'"), "{formatted}");
    let formatted = crate::fmt::format_stmt(
        &Parser::parse("QUERY 'x' FROM docs PARAMS (idf = WHERE tenant_id = 'acme') LIMIT 5;")
            .unwrap(),
    );
    assert!(
        formatted.contains("idf = WHERE tenant_id = 'acme'"),
        "{formatted}"
    );

    // JSON corpus objects and other non-filter values are rejected at parse.
    assert!(Parser::parse("QUERY 'x' FROM docs PARAMS (idf = 5) LIMIT 5;").is_err());
    assert!(Parser::parse("QUERY 'x' FROM docs PARAMS (idf = {foo: 1}) LIMIT 5;").is_err());
    assert!(Parser::parse(
        "QUERY 'x' FROM docs PARAMS (idf = {corpus: {must: [{key: 'status', match: {value: 'active'}}]}}) LIMIT 5;"
    )
    .is_err());
}

#[test]
fn shard_clause_parses_on_query_and_ctes_via_set_shard_key() {
    // Preferred path: SHARD in QQL
    let with_clause = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense LIMIT 10) \
         QUERY FUSION RRF FROM docs PREFETCH (c) SHARD 'acme' LIMIT 5;",
    )
    .unwrap();
    let Stmt::Query(q) = &with_clause else {
        panic!()
    };
    assert_eq!(q.shard_key.as_deref(), Some("acme"));

    // Host path after parse: property setter (recurses into CTEs)
    let mut stmt = Parser::parse(
        "WITH c AS (QUERY TEXT 'x' USING dense LIMIT 10) \
         QUERY FUSION RRF FROM docs PREFETCH (c) LIMIT 5;",
    )
    .unwrap();
    assert!(stmt.set_shard_key(Some("acme".into())));
    let Stmt::Query(q) = &stmt else { panic!() };
    assert_eq!(q.shard_key.as_deref(), Some("acme"));
    assert_eq!(q.ctes[0].query.shard_key.as_deref(), Some("acme"));
    assert!(stmt.set_shard_key(Some(String::new()))); // empty clears
    assert_eq!(stmt.shard_key(), None);
    assert!(
        !Parser::parse("SHOW COLLECTIONS")
            .unwrap()
            .set_shard_key(Some("x".into()))
    );
}

#[test]
fn mutation_shard_key_parses_from_qql() {
    let clear = Parser::parse("CLEAR PAYLOAD FROM docs WHERE id = 1 SHARD 'tenant-a';").unwrap();
    let Stmt::ClearPayload(c) = clear else {
        panic!("expected ClearPayload");
    };
    assert_eq!(c.shard_key.as_deref(), Some("tenant-a"));

    let del_vec =
        Parser::parse("DELETE VECTOR dense FROM docs WHERE id = 1 SHARD 'tenant-b';").unwrap();
    let Stmt::DeleteVector(d) = del_vec else {
        panic!("expected DeleteVector");
    };
    assert_eq!(d.shard_key.as_deref(), Some("tenant-b"));

    let upd_vec =
        Parser::parse("UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1 SHARD 'tenant-c';")
            .unwrap();
    let Stmt::UpdateVector(u) = upd_vec else {
        panic!("expected UpdateVector");
    };
    assert_eq!(u.shard_key.as_deref(), Some("tenant-c"));

    let upd_pay =
        Parser::parse("UPDATE docs SET PAYLOAD = {\"a\": 1} WHERE id = 1 SHARD 'tenant-d';")
            .unwrap();
    let Stmt::UpdatePayload(p) = upd_pay else {
        panic!("expected UpdatePayload");
    };
    assert_eq!(p.shard_key.as_deref(), Some("tenant-d"));

    let mut host = Parser::parse("CLEAR PAYLOAD FROM docs WHERE id = 2;").unwrap();
    assert!(host.set_shard_key(Some("injected".into())));
    assert_eq!(host.shard_key(), Some("injected"));
}

#[test]
fn cross_rerank_parses() {
    let stmt = Parser::parse(
        "WITH c AS (QUERY TEXT 'q' FROM docs USING dense LIMIT 50) \
         QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD body \
         FROM docs PREFETCH (c) LIMIT 10;",
    )
    .unwrap();
    match stmt {
        Stmt::Query(q) => match &q.expression {
            QueryExpr::CrossRerank {
                query,
                model,
                field,
                prefetch,
            } => {
                assert_eq!(query, "q");
                assert_eq!(model, "bge-reranker-base");
                assert_eq!(field.as_deref(), Some("body"));
                assert_eq!(prefetch.len(), 1);
            }
            other => panic!("expected CrossRerank, got {other:?}"),
        },
        other => panic!("expected query, got {other:?}"),
    }
}

#[test]
fn image_query_input_parses() {
    let stmt = Parser::parse(
        "QUERY IMAGE '/data/photo.jpg' MODEL 'clip-vision' FROM products USING image AS DENSE LIMIT 5;",
    )
    .unwrap();
    match stmt {
        Stmt::Query(q) => match &q.expression {
            QueryExpr::Nearest {
                input: QueryInput::Image { source, model },
                using: Some(u),
                ..
            } => {
                assert_eq!(source, "/data/photo.jpg");
                assert_eq!(model.as_deref(), Some("clip-vision"));
                assert_eq!(u.name, "image");
            }
            other => panic!("expected IMAGE nearest, got {other:?}"),
        },
        other => panic!("expected query, got {other:?}"),
    }
}

#[test]
fn upsert_using_image_parses() {
    let stmt = Parser::parse(
        "UPSERT INTO products VALUES {id: 1, image: '/a.jpg'} \
         USING IMAGE MODEL 'clip-vision' ON FIELD image INTO image;",
    )
    .unwrap();
    match stmt {
        Stmt::Upsert(u) => match &u.embedding {
            Some(EmbeddingSpec::Image {
                model,
                vector,
                field,
            }) => {
                assert_eq!(model.as_deref(), Some("clip-vision"));
                assert_eq!(vector.as_deref(), Some("image"));
                assert_eq!(field.as_deref(), Some("image"));
            }
            other => panic!("expected IMAGE embedding spec, got {other:?}"),
        },
        other => panic!("expected upsert, got {other:?}"),
    }
}

#[test]
fn using_as_multi_marks_dense_multivector() {
    let s =
        Parser::parse("QUERY TEXT 'search' FROM docs USING colbert AS MULTI LIMIT 10;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(
        q.expression,
        QueryExpr::Nearest {
            using: Some(crate::ast::VectorTarget {
                ref name,
                kind: Some(crate::ast::VectorKind::Dense),
                multi: true,
            }),
            ..
        } if name == "colbert"
    ));
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
fn removed_pre_v1_aliases_are_rejected() {
    for source in [
        "INSERT INTO docs VALUES {id: 1}",
        "BOOST ($score * 2)",
        "CREATE COLLECTION docs VECTORS (dense VECTOR (4, COSINE))",
        "CREATE COLLECTION docs (VECTOR (4, COSINE))",
        "CREATE COLLECTION docs (dense (4, COSINE))",
        "CREATE COLLECTION docs (dense VECTOR (4, COSINE) WITH VECTORS (on_disk = true))",
        "ALTER COLLECTION docs WITH QUANTIZE (type = 'scalar')",
        "CREATE SHARD 'tenant' ON COLLECTION docs",
        "QUERY TEXT 'x' FROM docs PARAMS (k = 30)",
        "QUERY TEXT 'x' FROM docs PARAMS (weights = [1.0])",
    ] {
        assert!(Parser::parse(source).is_err(), "{source}");
    }
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
        Parser::parse_all("SHOW COLLECTIONS; SHOW COLLECTION docs;")
            .unwrap()
            .len(),
        2
    );
    assert!(Parser::parse_all("SHOW COLLECTIONS SHOW COLLECTION docs").is_err());
}

#[test]
fn parse_upsert_on_field_and_multi_spec() {
    let stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello', title: 'world'} USING DENSE MODEL 'nomic' ON FIELD title INTO title_vec;",
    ).unwrap();
    match stmt {
        Stmt::Upsert(u) => match u.embedding.unwrap() {
            EmbeddingSpec::Dense {
                model,
                vector,
                field,
            } => {
                assert_eq!(model.as_deref(), Some("nomic"));
                assert_eq!(vector.as_deref(), Some("title_vec"));
                assert_eq!(field.as_deref(), Some("title"));
            }
            _ => panic!("expected Dense embedding spec"),
        },
        _ => panic!("expected Upsert statement"),
    }

    let multi_stmt = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello', title: 'world'} USING DENSE MODEL 'm1' ON FIELD text INTO dense, DENSE MODEL 'm2' ON FIELD title INTO title_vec;",
    ).unwrap();
    match multi_stmt {
        Stmt::Upsert(u) => match u.embedding.unwrap() {
            EmbeddingSpec::Multi(specs) => {
                assert_eq!(specs.len(), 2);
                match &specs[0] {
                    EmbeddingSpec::Dense {
                        model,
                        vector,
                        field,
                    } => {
                        assert_eq!(model.as_deref(), Some("m1"));
                        assert_eq!(vector.as_deref(), Some("dense"));
                        assert_eq!(field.as_deref(), Some("text"));
                    }
                    _ => panic!("expected Dense spec"),
                }
                match &specs[1] {
                    EmbeddingSpec::Dense {
                        model,
                        vector,
                        field,
                    } => {
                        assert_eq!(model.as_deref(), Some("m2"));
                        assert_eq!(vector.as_deref(), Some("title_vec"));
                        assert_eq!(field.as_deref(), Some("title"));
                    }
                    _ => panic!("expected Dense spec"),
                }
            }
            _ => panic!("expected Multi embedding spec"),
        },
        _ => panic!("expected Upsert statement"),
    }
}

#[test]
fn parse_upsert_with_dollar_and_pattern_strings() {
    let stmt = Parser::parse(
        r"UPSERT INTO qql_memory VALUES { id: 'abc', pattern_text: 'QUERY \$QUERY_TEXT FROM docs USING dense LIMIT \$LIMIT' };"
    ).unwrap();
    let Stmt::Upsert(u) = stmt else { panic!() };
    let (_, val) = &u.points[0].payload[0];
    match val {
        crate::ast::Value::Str(s) => {
            assert_eq!(s, "QUERY $QUERY_TEXT FROM docs USING dense LIMIT $LIMIT")
        }
        _ => panic!("expected string payload"),
    }

    let raw_stmt = Parser::parse(
        r"UPSERT INTO qql_memory VALUES { id: 'abc', pattern_text: r'QUERY $QUERY_TEXT FROM docs USING dense LIMIT $LIMIT' };"
    ).unwrap();
    let Stmt::Upsert(u_raw) = raw_stmt else {
        panic!()
    };
    let (_, val_raw) = &u_raw.points[0].payload[0];
    match val_raw {
        crate::ast::Value::Str(s) => {
            assert_eq!(s, "QUERY $QUERY_TEXT FROM docs USING dense LIMIT $LIMIT")
        }
        _ => panic!("expected string payload"),
    }

    let raw_backslash_stmt = Parser::parse(
        r"UPSERT INTO qql_memory VALUES { id: 'abc', pattern_text: r'path\to\$file' };",
    )
    .unwrap();
    let Stmt::Upsert(u_raw_bs) = raw_backslash_stmt else {
        panic!()
    };
    let (_, val_raw_bs) = &u_raw_bs.points[0].payload[0];
    match val_raw_bs {
        crate::ast::Value::Str(s) => {
            assert_eq!(s, r"path\to\$file");
        }
        _ => panic!("expected string payload"),
    }

    let triple_stmt = Parser::parse(
        "UPSERT INTO qql_memory VALUES { id: 'abc', pattern_text: '''QUERY '$QUERY_TEXT'\nFROM berlin_airbnb\nLIMIT $LIMIT;''' };"
    ).unwrap();
    let Stmt::Upsert(u_triple) = triple_stmt else {
        panic!()
    };
    let (_, val_triple) = &u_triple.points[0].payload[0];
    match val_triple {
        crate::ast::Value::Str(s) => {
            assert_eq!(s, "QUERY '$QUERY_TEXT'\nFROM berlin_airbnb\nLIMIT $LIMIT;")
        }
        _ => panic!("expected string payload"),
    }
}

#[test]
fn triple_quoted_strings_preserve_backslash_verbatim() {
    let stmt = Parser::parse(r"UPSERT INTO docs VALUES {id: 1, text: '''a\nb'''};").unwrap();
    let Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert")
    };
    let (_, value) = &upsert.points[0].payload[0];
    match value {
        crate::ast::Value::Str(s) => {
            // Backslash is content, not an escape: the value is `a\nb`
            // (backslash + n), never a real newline.
            assert_eq!(s, r"a\nb");
        }
        _ => panic!("expected string payload"),
    }
}

#[test]
fn triple_quoted_strings_preserve_doubled_quotes_verbatim() {
    let stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: '''it''s'''};").unwrap();
    let Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert")
    };
    let (_, value) = &upsert.points[0].payload[0];
    match value {
        crate::ast::Value::Str(s) => assert_eq!(s, "it''s"),
        _ => panic!("expected string payload"),
    }
}

#[test]
fn triple_quoted_double_delimited_strings_are_verbatim() {
    let stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: \"\"\"a\\nb\"\"\"};").unwrap();
    let Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert")
    };
    let (_, value) = &upsert.points[0].payload[0];
    match value {
        crate::ast::Value::Str(s) => assert_eq!(s, r"a\nb"),
        _ => panic!("expected string payload"),
    }
}

#[test]
fn four_quotes_decode_to_single_apostrophe() {
    let stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: ''''};").unwrap();
    let Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert")
    };
    let (_, value) = &upsert.points[0].payload[0];
    match value {
        crate::ast::Value::Str(s) => assert_eq!(s, "'"),
        _ => panic!("expected string payload"),
    }
}

#[test]
fn empty_triple_quoted_string_decodes_to_empty() {
    let stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: ''''''};").unwrap();
    let Stmt::Upsert(upsert) = stmt else {
        panic!("expected upsert")
    };
    let (_, value) = &upsert.points[0].payload[0];
    match value {
        crate::ast::Value::Str(s) => assert_eq!(s, ""),
        _ => panic!("expected string payload"),
    }
}

/// F-3: `Stmt` serialization must round-trip through its `Deserialize`. The
/// canonical serialized form of the unit variant is the empty-object tag
/// `{"ShowCollections": {}}` (kept for consumers that emit that shape); the
/// manual deserializer also accepts the derived string form `"ShowCollections"`,
/// so both directions of the contract work.
#[cfg(feature = "json")]
#[test]
fn stmt_serde_round_trips_through_json() {
    use crate::ast::Stmt;
    // One representative statement per `Stmt` variant.
    let sources = [
        "QUERY TEXT 'hello' FROM docs LIMIT 10;",
        "SCROLL FROM docs LIMIT 10;",
        "UPSERT INTO docs VALUES {id: 1, title: 'x'};",
        "CREATE COLLECTION docs (dense VECTOR (4, COSINE));",
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
        "DROP INDEX ON COLLECTION docs FOR title;",
        "CREATE SHARD KEY 'a' ON COLLECTION docs WITH (shards_number = 2);",
        "DROP SHARD KEY 'a' ON COLLECTION docs;",
        "ALTER COLLECTION docs WITH VECTOR (on_disk = true);",
        "DROP COLLECTION docs;",
        "SHOW COLLECTIONS;",
        "SHOW COLLECTION docs;",
        "SHOW SHARD KEYS ON COLLECTION docs;",
        "DELETE FROM docs WHERE id = 1;",
        "CLEAR PAYLOAD FROM docs WHERE id = 1;",
        "DELETE PAYLOAD title FROM docs WHERE id = 1;",
        "DELETE VECTOR dense FROM docs WHERE id = 1;",
        "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
        "UPDATE docs SET PAYLOAD = {a: 1} WHERE id = 1;",
        "COUNT FROM docs WHERE active = true WITH (exact = true);",
    ];
    for source in sources {
        let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("{source} should parse: {e}"));
        let json = serde_json::to_string(&stmt)
            .unwrap_or_else(|e| panic!("{source} should serialize: {e}"));
        let back: Stmt = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("{source} should deserialize from {json}: {e}"));
        assert_eq!(stmt, back, "round-trip mismatch for: {source}");
    }

    // The canonical serialized form is the empty-object tag (matches the
    // conformance snapshot and the JS/Python bindings' output).
    assert_eq!(
        serde_json::to_string(&Stmt::ShowCollections).unwrap(),
        "{\"ShowCollections\":{}}"
    );
    // Both the canonical tag and the derived string form deserialize.
    assert_eq!(
        serde_json::from_str::<Stmt>(r#"{"ShowCollections":{}}"#).unwrap(),
        Stmt::ShowCollections
    );
    assert_eq!(
        serde_json::from_str::<Stmt>("\"ShowCollections\"").unwrap(),
        Stmt::ShowCollections
    );
}

#[test]
fn implicit_array_vector_literal_parses() {
    let stmt1 = Parser::parse("QUERY [0.1, 0.2, 0.3] FROM docs;").unwrap();
    let stmt2 = Parser::parse("QUERY VECTOR [0.1, 0.2, 0.3] FROM docs;").unwrap();
    assert_eq!(stmt1, stmt2);

    let stmt3 = Parser::parse("QUERY [[0.1, 0.2], [0.3, 0.4]] FROM docs;").unwrap();
    let stmt4 = Parser::parse("QUERY VECTOR [[0.1, 0.2], [0.3, 0.4]] FROM docs;").unwrap();
    assert_eq!(stmt3, stmt4);
}
