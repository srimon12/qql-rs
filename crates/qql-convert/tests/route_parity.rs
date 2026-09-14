//! Route parity: the converter's output must plan back to the same REST route.
//!
//! For every corpus statement:
//!
//! ```text
//! stmt --qql_plan::plan--> op --to_rest_route--> Route{method,path,body}
//!      --qql-convert--> QQL --parse--> stmt' --plan--> op' --to_rest_route--> Route'
//! assert Route' == Route (method, path, body)
//! ```
//!
//! This proves the AST decode is the exact inverse of the planner for every
//! shape QQL itself can emit, against the same contract the runtime uses.
//!
//! REST query-string parameters (`wait`, `timeout`, `consistency`) ride the
//! wrapped `"query"` object so the converter can recover `WAIT` and
//! `PARAMS (timeout / consistency)`.

use qql_convert::convert;
use qql_core::fmt::format_stmt;
use qql_core::params_json::bind_str_with_params;
use qql_core::parser::Parser;
use qql_plan::{RestProjectionError, plan, to_rest_route};

/// Plan a source statement, convert its REST body, and compare routes.
fn assert_parity(source: &str) {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    let body = route.body_json();
    let mut wrapped = serde_json::json!({
        "method": route.method.as_str(),
        "path": route.path,
        "body": body,
    });
    if !route.query.is_empty() {
        let mut query = serde_json::Map::new();
        for (key, value) in &route.query {
            query.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        wrapped["query"] = serde_json::Value::Object(query);
    }
    let wrapped = wrapped.to_string();

    let emitted =
        convert(&wrapped, None).unwrap_or_else(|e| panic!("convert {source} ({wrapped}): {e}"));
    assert_eq!(
        emitted.len(),
        1,
        "{source} must convert to exactly one statement: {emitted:?}"
    );

    let reparsed = Parser::parse(&format!("{};", emitted[0]))
        .unwrap_or_else(|e| panic!("reparse {}: {e}", emitted[0]));
    assert_eq!(
        format_stmt(&reparsed),
        emitted[0],
        "converted {source} is not canonical"
    );
    let replanned = plan(&reparsed).unwrap_or_else(|e| panic!("replan {}: {e}", emitted[0]));
    let reroute =
        to_rest_route(&replanned).unwrap_or_else(|e| panic!("reroute {}: {e:?}", emitted[0]));

    assert_eq!(reroute.method.as_str(), route.method.as_str(), "{source}");
    assert_eq!(reroute.path, route.path, "{source}");
    assert_eq!(reroute.query, route.query, "{source}");
    assert_eq!(
        reroute.body_json(),
        body,
        "{source}\nemitted: {}\nreroute: {:?}",
        emitted[0],
        reroute.body_json()
    );
}

/// Query-expression corpus: every `QueryExpr` variant QQL can wire, plus the
/// controls the runtime actually serializes.
const QUERY_CORPUS: &[&str] = &[
    // 1. POINTS
    "QUERY POINTS (1, 'pt-2') FROM docs SHARD 'acme' WITH PAYLOAD false WITH VECTOR (dense);",
    // 2. NEAREST
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE k = 1 LIMIT 5 OFFSET 2;",
    "QUERY VECTOR [0.1, 0.2] FROM docs USING dense SCORE THRESHOLD 0.5 LIMIT 5;",
    "QUERY {indices: [1, 5], values: [0.5, 0.8]} FROM docs USING sparse AS SPARSE LIMIT 5;",
    "QUERY IMAGE 'https://x/y.jpg' MODEL 'clip' FROM docs USING image LIMIT 5;",
    "QUERY POINT 42 FROM docs USING dense LIMIT 5;",
    "QUERY MMR TEXT 'q' MODEL 'e5' DIVERSITY 0.5 CANDIDATES 20 FROM docs USING dense LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING dense WITH PAYLOAD INCLUDE (title, url) WITH VECTOR true LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING dense PARAMS (hnsw_ef = 64, exact = false, indexed_only = true, acorn = true, max_selectivity = 0.4) LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING dense PARAMS (timeout = 30, consistency = majority) LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING dense PARAMS (consistency = 2) LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING sparse PARAMS (idf = 'global') LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs USING sparse PARAMS (idf = WHERE tenant = 'acme') LIMIT 5;",
    "QUERY TEXT 'q' MODEL 'e5' FROM docs SHARD 101 LIMIT 5;",
    // 3. RECOMMEND
    "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY best_score FROM docs USING dense LIMIT 10;",
    "QUERY RECOMMEND POSITIVE (VECTOR [0.1, 0.2]) NEGATIVE (VECTOR [0.3, 0.4]) FROM docs USING dense LIMIT 10;",
    "QUERY RECOMMEND POSITIVE (TEXT 'hello' MODEL 'e5', POINT 7) FROM docs USING dense LIMIT 10;",
    // 4. CONTEXT
    "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2, POSITIVE VECTOR [0.1] NEGATIVE VECTOR [0.2]) FROM docs USING dense LIMIT 10;",
    // 5. DISCOVER
    "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
    "QUERY DISCOVER TARGET TEXT 'anchor' MODEL 'e5' CONTEXT (POSITIVE VECTOR [0.1] NEGATIVE VECTOR [0.2]) FROM docs USING dense LIMIT 10;",
    // 6. ORDER BY
    "QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20;",
    // 7. SAMPLE
    "QUERY SAMPLE RANDOM FROM docs LIMIT 5;",
    // 8. FUSION
    "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 100), b AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (a, b) LIMIT 10;",
    "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) QUERY FUSION RRF FROM docs PREFETCH (a) PARAMS (rrf_k = 60, rrf_weights = [0.4]) LIMIT 10;",
    "QUERY FUSION DBSF FROM docs PREFETCH (QUERY TEXT 'x' MODEL 'e5' USING dense LIMIT 20, QUERY TEXT 'x' MODEL 'e5' USING sparse LIMIT 20) LIMIT 10;",
    "QUERY FUSION RRF FROM docs PREFETCH (QUERY TEXT 'x' MODEL 'e5' USING dense WHERE k = 1 PARAMS (hnsw_ef = 32) SCORE THRESHOLD 0.2 LIMIT 20) LIMIT 10;",
    // 9. FORMULA
    "QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0) FROM docs LIMIT 5;",
    "QUERY FORMULA CASE WHEN status = 'active' THEN score * 2 ELSE score END FROM docs LIMIT 5;",
    "QUERY FORMULA EXP_DECAY(judgment_date, TARGET = '2026-09-04T00:00:00Z', SCALE = 630720000, MIDPOINT = 0.5) FROM docs LIMIT 5;",
    "QUERY FORMULA GEO_DISTANCE(52.5, 13.4, location) + MATCH(is_superhost, true) FROM docs LIMIT 5;",
    "QUERY FORMULA (score / views [DEFAULT = 1.0]) FROM docs LIMIT 5;",
    "QUERY FORMULA MAX(score, bonus) + MIN(rank) FROM docs LIMIT 5;",
    "QUERY FORMULA POW(score, 2) FROM docs LIMIT 5;",
    "QUERY FORMULA DATETIME('2024-01-01T00:00:00Z') FROM docs LIMIT 5;",
    "QUERY FORMULA DATETIME_KEY('created_at') FROM docs LIMIT 5;",
    "QUERY FORMULA MATCH_ANY(tags, ['a', 'b']) FROM docs LIMIT 5;",
    "QUERY FORMULA LIN_DECAY(score, TARGET = 0.5) FROM docs LIMIT 5;",
    // 10. RELEVANCE FEEDBACK
    "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5), (POINT 44, -0.2)) STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs USING dense LIMIT 10;",
    "QUERY RELEVANCE FEEDBACK TARGET VECTOR [0.1] FEEDBACK ((VECTOR [0.2], 0.5)) STRATEGY NAIVE (a = 1.0, b = 1.0, c = 0.0) FROM docs USING dense LIMIT 10;",
    // 11. HYBRID (plans as fusion + two prefetches)
    "QUERY HYBRID TEXT 'ai search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
    "QUERY HYBRID TEXT 'q' MODEL 'bge' DENSE dense SPARSE sparse FUSION DBSF FROM docs LIMIT 5;",
    // 12. RERANK (plans as nearest with model + prefetch)
    "QUERY RERANK TEXT 'travel tips' MODEL 'colbert-v2' FROM docs USING colbert PREFETCH (QUERY TEXT 'travel tips' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
    // GROUP BY
    "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics LIMIT 20;",
    // Filters on a query
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE NOT (a = 1 OR b MATCH ANY ('x', 'y')) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE loc GEO_RADIUS {center: {lat: 52.5, lon: 13.4}, radius: 1000.0} LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE NESTED('items', price >= 10) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE SLICE (4, 2) AND HAS_VECTOR dense LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE created BETWEEN '2024-01-01T00:00:00Z' AND '2024-12-31T00:00:00Z' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE tag VALUES_COUNT >= 2 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title MATCH PHRASE 'exact phrase' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title MATCH PREFIX 'pre' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE a IS NULL OR b IS EMPTY LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE id = 7 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE id IN (1, 2, 'pt-3') LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE age BETWEEN 18 AND 65 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE rating = 4.5 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title MATCH ANY (1, 2) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title MATCH TOKENS 'red shoes' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE tags MATCH EXCEPT ('a', 'b') LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE code MATCH EXCEPT (1, 2) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE MIN SHOULD 2 (a = 1, b = 2) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE MIN SHOULD 1 (title MATCH TOKENS 'a b', tags MATCH EXCEPT (1, 2)) LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE big = 18446744073709551615 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE n > 18446744073709551615 LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE name > 'm' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE name BETWEEN 'a' AND 'm' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title MATCH 'text with spaces' LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE title = true LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WITH PAYLOAD true LIMIT 5;",
    "QUERY ORDER BY rank ASC FROM docs LIMIT 5;",
    // W2: ORDER BY paging origin.
    "QUERY ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' FROM docs LIMIT 20;",
    "QUERY ORDER BY score ASC START FROM 100 FROM docs LIMIT 5;",
    // W2: group lookup with payload/vector selectors.
    "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 20;",
    // W2: prefetch LOOKUP with shard routing.
    "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) QUERY FUSION RRF FROM docs PREFETCH (a LOOKUP FROM docs2 VECTOR dense SHARD 'acme') LIMIT 10;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs SHARD 'acme' GROUP BY topic LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE loc GEO_BBOX {top_left: {lat: 1.0, lon: 2.0}, bottom_right: {lat: 3.0, lon: 4.0}} LIMIT 5;",
    "QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE loc GEO_POLYGON {exterior: [{lat: 0.0, lon: 0.0}, {lat: 1.0, lon: 0.0}, {lat: 1.0, lon: 1.0}], interiors: [[{lat: 0.1, lon: 0.1}, {lat: 0.2, lon: 0.1}, {lat: 0.2, lon: 0.2}]]} LIMIT 5;",
    // Inference inputs with OPTIONS / InferenceObject (w4-inference-config).
    "QUERY TEXT 'x' MODEL 'e5' OPTIONS {temperature: 0.5} FROM docs USING dense LIMIT 5;",
    "QUERY IMAGE 'https://x/y.jpg' MODEL 'clip' OPTIONS {size: 512} FROM docs USING image LIMIT 5;",
    "QUERY OBJECT {prompt: 'x', n: 2} MODEL 'm' FROM docs USING dense LIMIT 5;",
    "QUERY OBJECT {a: 1} MODEL 'm' OPTIONS {k: true} FROM docs USING dense LIMIT 5;",
    "QUERY OBJECT {a: 1} FROM docs USING dense LIMIT 5;",
];

/// Retrieval / mutation corpus (SCROLL, COUNT, FACET, UPSERT, DELETE, …).
const MUTATION_CORPUS: &[&str] = &[
    "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
    "SCROLL FROM docs AFTER 7 SHARD 'acme' LIMIT 10;",
    "SCROLL FROM docs AFTER '550e8400-e29b-41d4-a716-446655440000' LIMIT 10;",
    "SCROLL FROM docs AFTER 'not-a-uuid' LIMIT 10;",
    "SCROLL FROM docs WITH VECTOR (dense) LIMIT 5;",
    // W2: SCROLL ordering, payload selectors, and the combined shape.
    "SCROLL FROM docs ORDER BY created_at DESC LIMIT 10;",
    "SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10;",
    "SCROLL FROM docs WITH PAYLOAD false LIMIT 10;",
    "SCROLL FROM docs WITH PAYLOAD INCLUDE (title, url) LIMIT 10;",
    "SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10;",
    "SCROLL FROM docs WHERE status = 'active' AFTER 7 ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' SHARD 'acme' WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 10;",
    "COUNT FROM docs WHERE status = 'active';",
    "COUNT FROM docs SHARD 101 WITH (exact = false);",
    "COUNT FROM docs WHERE status = 'active' WITH (exact = true);",
    "FACET category FROM docs WHERE active = true LIMIT 10;",
    "FACET category FROM docs LIMIT 5 EXACT true;",
    "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second', category: 'science'};",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2], title: 'hello'};",
    "UPSERT INTO docs VALUES {id: 1, vector: {dense: [0.1], sparse: {indices: [1], values: [0.5]}}};",
    "UPSERT INTO docs VALUES {id: 1, vector: [[0.1, 0.2], [0.3, 0.4]]};",
    "UPSERT INTO docs VALUES {id: 'pt-1', payload: true} SHARD 'acme';",
    "DELETE FROM docs WHERE id = 1;",
    "DELETE FROM docs WHERE id = 1 WAIT true;",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT true;",
    "DELETE FROM docs WHERE id IN (1, 2, 'pt-3');",
    "DELETE FROM docs WHERE category = 'archived' SHARD 101;",
    "CLEAR PAYLOAD FROM docs WHERE k = 1;",
    "CLEAR PAYLOAD FROM docs WHERE id IN (1, 2);",
    "DELETE PAYLOAD a, b FROM docs WHERE id IN (1, 2);",
    "DELETE PAYLOAD a FROM docs WHERE id = 1;",
    "DELETE VECTOR dense, sparse FROM docs WHERE k = 1;",
    "UPDATE docs SET VECTOR = [0.5, 0.6] WHERE id = 1;",
    "UPDATE docs SET VECTOR dense = [0.5] WHERE id = 1;",
    "UPDATE docs SET VECTOR = {dense: [0.5], sparse: {indices: [1], values: [0.8]}} WHERE id = 1;",
    "UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1]}, {id: 2, vector: {dense: [0.2]}};",
    "UPDATE docs SET PAYLOAD = {a: 1, b: 'x'} WHERE id = 1;",
    "UPDATE docs SET PAYLOAD = {a: 1} WHERE k = 1;",
    "UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1;",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active';",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only;",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE update_only;",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE upsert;",
    "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active' UPDATE MODE update_only;",
    // Per-point inference vectors (w4-inference-config).
    "UPSERT INTO docs VALUES {id: 1, vector: {text: 'hello', model: 'm'}};",
    "UPSERT INTO docs VALUES {id: 1, vector: {dense: {image: 'https://x/y.jpg', model: 'c'}, sparse: {indices: [1], values: [0.5]}}};",
    "UPSERT INTO docs VALUES {id: 1, vector: {object: {a: 1}, model: 'm', options: {k: 1}}};",
    "UPDATE docs SET VECTOR VALUES {id: 1, vector: {text: 'hello', model: 'm'}};",
];

/// DDL corpus (CREATE/ALTER/DROP/index/shard keys/quotas).
const DDL_CORPUS: &[&str] = &[
    "CREATE COLLECTION docs (dense VECTOR(384, COSINE));",
    "CREATE COLLECTION docs (dense VECTOR(128, DOT), image VECTOR(512, EUCLID) WITH HNSW (m = 16, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.99));",
    "CREATE COLLECTION docs (sparse SPARSE WITH SPARSE (modifier = 'idf', full_scan_threshold = 100, on_disk = true));",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', query_encoding = 'scalar4bits', always_ram = true));",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = false));",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH QUANTIZATION (type = 'turbo', bits = 1.5, always_ram = true));",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH VECTOR (on_disk = true, datatype = 'float16'));",
    "CREATE COLLECTION docs WITH HNSW (m = 32, payload_m = 8, on_disk = true, memory = 'cached') WITH OPTIMIZERS (deleted_threshold = 0.2, vacuum_min_vector_number = 100, default_segment_number = 4, max_segment_size = 1000, memmap_threshold = 100, indexing_threshold = 20000, flush_interval_sec = 5, max_optimization_threads = 'auto', prevent_unoptimized = true);",
    "CREATE COLLECTION docs WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = true, payload_memory = 'cached', shard_number = 4, sharding_method = 'custom');",
    "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95);",
    "ALTER COLLECTION docs WITH OPTIMIZERS (indexing_threshold = 500);",
    "ALTER COLLECTION docs WITH HNSW (m = 8) WITH PARAMS (replication_factor = 3);",
    "ALTER COLLECTION docs WITH QUANTIZATION (disabled = true);",
    "ALTER COLLECTION docs WITH QUANTIZATION (type = 'binary', always_ram = true);",
    "ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 8), VECTOR (on_disk = true), QUANTIZATION (type = 'scalar', quantile = 0.5));",
    "ALTER COLLECTION docs WITH SPARSE sparse (SPARSE (modifier = 'none', full_scan_threshold = 50));",
    "DROP COLLECTION docs;",
    "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword;",
    "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword WAIT true;",
    "CREATE INDEX ON COLLECTION docs FOR loc TYPE geo;",
    "CREATE INDEX ON COLLECTION docs FOR tenant TYPE keyword WITH (is_tenant = true, prefix = true, memory = 'cached');",
    "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (lowercase = true, ascii_folding = true, phrase_matching = true, min_token_len = 2, max_token_len = 10, tokenizer = 'word', stemmer = 'english', stopwords = ['the', 'a']);",
    "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = 'english');",
    "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = {languages: ['english', 'german'], custom: ['foo']});",
    "CREATE INDEX ON COLLECTION docs FOR n TYPE integer WITH (lookup = true, range = false, is_principal = true);",
    "DROP INDEX ON COLLECTION docs FOR city;",
    "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 3, replication_factor = 2);",
    "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2, placement = [1, 2], initial_state = 'Active');",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 32, wal_segments_ahead = 2, wal_retain_closed = 1);",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH STRICT_MODE (enabled = true, max_query_limit = 100);",
    "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH METADATA (owner = 'team', version = 3);",
    "ALTER COLLECTION docs WITH STRICT_MODE (enabled = false, search_allow_exact = true);",
    "ALTER COLLECTION docs WITH METADATA (owner = 'team');",
    "CREATE SHARD KEY 101 ON COLLECTION docs;",
    "DROP SHARD KEY 'acme' ON COLLECTION docs;",
    "SET QUOTA (enabled = true, max_resident_memory_percent = 80, max_disk_usage_percent = 90, release_margin_percent = 5);",
    "SHOW COLLECTIONS;",
    "SHOW COLLECTION docs;",
    "SHOW SHARD KEYS ON COLLECTION docs;",
    "SHOW QUOTAS;",
];

#[test]
fn query_corpus_routes_round_trip() {
    for source in QUERY_CORPUS {
        assert_parity(source);
    }
}

const BATCH_CORPUS: &[&str] = &[
    "BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 3; }",
    "BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 3; } PARAMS (timeout = 30, consistency = majority)",
    "BATCH { UPSERT INTO docs VALUES {id: 1, vector: [0.1]}; DELETE FROM docs WHERE id = 2; }",
    "BATCH { UPSERT INTO docs VALUES {id: 1, vector: [0.1]}; DELETE FROM docs WHERE id = 2; } WAIT false",
    "BATCH { UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1; UPDATE docs SET PAYLOAD = {b: 2} WHERE id = 2; }",
    "BATCH { UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' OVERWRITE WHERE id = 1; }",
];

#[test]
fn batch_corpus_routes_round_trip() {
    for source in BATCH_CORPUS {
        assert_parity(source);
    }
}

#[test]
fn mutation_corpus_routes_round_trip() {
    for source in MUTATION_CORPUS {
        assert_parity(source);
    }
}

#[test]
fn ddl_corpus_routes_round_trip() {
    for source in DDL_CORPUS {
        assert_parity(source);
    }
}

#[test]
fn cross_rerank_is_client_side_and_has_no_rest_route() {
    let stmt = Parser::parse(
        "QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD body FROM docs \
         PREFETCH (QUERY TEXT 'q' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
    )
    .expect("parse cross rerank");
    let op = plan(&stmt).expect("plan cross rerank");
    match to_rest_route(&op) {
        Err(RestProjectionError::ClientSideOnly { stmt_type }) => {
            assert_eq!(stmt_type, "cross_rerank");
        }
        other => panic!("CROSS RERANK must be client-side: {other:?}"),
    }
}

#[test]
fn bench_corpus_routes_round_trip() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/queries.json");
    let raw = std::fs::read_to_string(path).expect("read bench/queries.json");
    let corpus: serde_json::Value = serde_json::from_str(&raw).expect("parse bench/queries.json");
    let queries = corpus["queries"].as_array().expect("queries array");
    let mut covered = 0;
    for entry in queries {
        let source = entry["qql"].as_str().expect("qql string");
        // Bind parameters when the corpus entry declares them.
        let bound: String = match entry.get("params") {
            Some(params) => bind_str_with_params(source, params, false).expect("bind params"),
            None => source.to_string(),
        };
        for single in bound.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let stmt = Parser::parse(&format!("{single};"))
                .unwrap_or_else(|e| panic!("parse bench query {single}: {e}"));
            if plan(&stmt).is_err() {
                panic!("bench query must plan: {single}");
            }
            assert_parity(&format!("{single};"));
            covered += 1;
        }
    }
    assert!(covered >= 13, "bench corpus should cover every entry");
}
