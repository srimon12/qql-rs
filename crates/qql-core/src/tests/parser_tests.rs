use crate::ast::{EmbeddingSpec, FusionMethod, QueryCollection, QueryExpr, QueryInput, Stmt};
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
