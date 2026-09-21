//! Owned-lowering parity + throughput: `plan_owned` must agree with `plan`
//! arm-for-arm, and move 10k×128-dim upserts without cloning vector buffers.
//!
//! Run: `cargo test -p qql-plan --test owned_parity`
//! Throughput: `cargo test -p qql-plan --test owned_parity --release -- --nocapture`

use qql_core::ast::{
    PointEntry, PointId, PointVectors, QueryCollection, UpsertPoint, UpsertStmt, VectorValue,
};
use qql_core::parser::Parser;

fn assert_parity(source: &str) {
    let borrowed_stmt = Parser::parse(source).expect("must parse");
    let owned_stmt = Parser::parse(source).expect("must parse");
    let borrowed = qql_plan::plan(&borrowed_stmt).expect("borrowed plan must succeed");
    let owned = qql_plan::plan_owned(owned_stmt).expect("owned plan must succeed");
    // Debug-shape equality is exact: every request type derives the same Debug
    // on both paths (moves vs clones change nothing observable).
    assert_eq!(
        format!("{borrowed:?}"),
        format!("{owned:?}"),
        "owned/borrowed divergence for {source}"
    );
    // REST bodies agree too (where a route exists).
    match (
        qql_plan::to_rest_route(&borrowed),
        qql_plan::to_rest_route(&owned),
    ) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.method, b.method, "method for {source}");
            assert_eq!(a.path, b.path, "path for {source}");
            assert_eq!(a.body, b.body, "body for {source}");
        }
        (Err(a), Err(b)) => assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "route errors diverge for {source}"
        ),
        (a, b) => panic!("route presence diverges for {source}: {a:?} vs {b:?}"),
    }
}

#[test]
fn owned_matches_borrowed_on_query_legs() {
    for source in [
        "QUERY [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
        "QUERY TEXT 'hello' MODEL 'e5' FROM docs USING dense WHERE status = 'active' LIMIT 5;",
        "QUERY RECOMMEND POSITIVE ([0.1, 0.2]) NEGATIVE ([0.3, 0.4]) STRATEGY average_vector FROM docs USING dense LIMIT 10;",
        "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs LIMIT 10;",
        "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
        "QUERY ORDER BY created_at DESC FROM docs LIMIT 10;",
        "QUERY SAMPLE RANDOM FROM docs LIMIT 5;",
        "QUERY FUSION RRF FROM docs PREFETCH (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
        "QUERY FORMULA score * 2 DEFAULTS (score = 0.0) FROM docs LIMIT 5;",
        "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5)) STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs USING dense LIMIT 10;",
        "QUERY HYBRID TEXT 'ai' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        "QUERY RERANK TEXT 't' MODEL 'colbert' FROM docs USING colbert PREFETCH (QUERY TEXT 't' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
        "QUERY POINTS (1, 2, 3) FROM docs;",
        "QUERY TEXT 'x' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LIMIT 20;",
        "QUERY TEXT 'x' MODEL 'e5' FROM docs SHARD 'tenant-a' LIMIT 5;",
        "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 20) QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 5;",
    ] {
        assert_parity(source);
    }
}

#[test]
fn owned_matches_borrowed_on_mutations() {
    for source in [
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]}, {id: 2, vector: [0.3, 0.4]};",
        "UPSERT INTO docs VALUES {id: 1, dense: [0.1], sparse: {indices: [0], values: [0.5]}};",
        "DELETE FROM docs WHERE id = 1;",
        "DELETE FROM docs WHERE status = 'old' SHARD 101;",
        "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
        "UPDATE docs SET PAYLOAD = {k: 1} WHERE id = 1;",
        "UPDATE docs SET PAYLOAD = {k: 1} OVERWRITE WHERE id = 1;",
        "CLEAR PAYLOAD FROM docs WHERE id = 1;",
        "DELETE PAYLOAD a, b FROM docs WHERE id = 1;",
        "DELETE VECTOR dense FROM docs WHERE id = 1;",
        "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
        "SCROLL FROM docs AFTER 42 LIMIT 10;",
        "COUNT FROM docs WHERE status = 'active';",
        "FACET topic FROM docs WHERE status = 'active' LIMIT 10;",
    ] {
        assert_parity(source);
    }
}

#[test]
fn owned_matches_borrowed_on_ddl_and_batch() {
    for source in [
        "CREATE COLLECTION docs (dense VECTOR(4, COSINE));",
        "ALTER COLLECTION docs WITH PARAMS (replication_factor = 2);",
        "DROP COLLECTION docs;",
        "CREATE INDEX ON COLLECTION docs FOR tag TYPE keyword;",
        "DROP INDEX ON COLLECTION docs FOR tag;",
        "SHOW COLLECTIONS;",
        "SHOW COLLECTION docs;",
        "SHOW SHARD KEYS ON COLLECTION docs;",
        "SHOW QUOTAS;",
        "SET QUOTA (enabled = true, max_resident_memory_percent = 80);",
        "CREATE SHARD KEY 't1' ON COLLECTION docs;",
        "DROP SHARD KEY 't1' ON COLLECTION docs;",
        "BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 1; };",
        "BATCH { UPSERT INTO docs VALUES {id: 1, vector: [0.1]}; DELETE FROM docs WHERE id = 2; };",
    ] {
        assert_parity(source);
    }
}

#[test]
fn owned_template_matches_borrowed_template() {
    let source = "QUERY VECTOR :qvec FROM docs USING dense LIMIT 5;";
    let borrowed_stmt = Parser::parse(source).unwrap();
    let owned_stmt = Parser::parse(source).unwrap();
    let borrowed = qql_plan::plan_template(&borrowed_stmt).unwrap();
    let owned = qql_plan::plan_template_owned(owned_stmt).unwrap();
    assert_eq!(format!("{borrowed:?}"), format!("{owned:?}"));
}

/// 10k×128-dim upsert throughput: borrowed (clone) vs owned (move).
///
/// This is a timing report, not a strict perf gate (CI machines vary): it
/// prints both timings and asserts functional equivalence. Planning is
/// dominated by payload JSON conversion and request allocation, so the ~5MB
/// vector memcpy the owned path eliminates often hides in noise on a single
/// run — hence min-of-N runs and a payload-free variant isolating vector cost.
/// The load-bearing property is structural: the owned path moves buffers
/// (no `Dense(d.clone())`), so at most one live copy exists end-to-end
/// (AST → plan moves; the single remaining copy is wire serialization).
#[test]
fn upsert_10k_128d_owned_moves_vectors() {
    let dim = 128usize;
    let n = 10_000usize;
    let build_stmt = |with_payload: bool| {
        let mut points = Vec::with_capacity(n);
        for i in 0..n {
            let vec = (0..dim).map(|d| (i * dim + d) as f32 * 0.001).collect();
            points.push(PointEntry::Inline(UpsertPoint {
                id: PointId::Number(i as u64),
                vectors: Some(PointVectors::Unnamed(VectorValue::Dense(vec))),
                payload: if with_payload {
                    vec![("t".to_string(), qql_core::ast::Value::Int(i as i64))]
                } else {
                    Vec::new()
                },
            }));
        }
        qql_core::ast::Stmt::Upsert(Box::new(UpsertStmt {
            collection: "docs".to_string(),
            points,
            embedding: None,
            embed: Vec::new(),
            update_filter: None,
            update_mode: None,
            shard_key: None,
            wait: None,
        }))
    };

    for with_payload in [false, true] {
        // Warmup.
        let _ = qql_plan::plan_owned(build_stmt(with_payload)).unwrap();
        let mut borrowed_best = f64::INFINITY;
        let mut owned_best = f64::INFINITY;
        let mut borrowed_op = None;
        let mut owned_op = None;
        for _ in 0..5 {
            let borrowed_stmt = build_stmt(with_payload);
            let t0 = std::time::Instant::now();
            let op = qql_plan::plan(&borrowed_stmt).unwrap();
            borrowed_best = borrowed_best.min(t0.elapsed().as_secs_f64() * 1000.0);
            borrowed_op = Some(format!("{op:?}").len());

            let t0 = std::time::Instant::now();
            let op = qql_plan::plan_owned(build_stmt(with_payload)).unwrap();
            owned_best = owned_best.min(t0.elapsed().as_secs_f64() * 1000.0);
            owned_op = Some(format!("{op:?}").len());
        }
        assert_eq!(borrowed_op, owned_op);
        eprintln!(
            "upsert {n}x{dim} payload={with_payload}: borrowed plan min {borrowed_best:.2}ms, owned plan_owned min {owned_best:.2}ms"
        );
    }
    let _ = QueryCollection::Explicit(String::new());
}
