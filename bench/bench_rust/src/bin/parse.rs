use std::time::Instant;

const QUERIES: &[(&str, &str)] = &[
    ("Simple", "QUERY 'search' FROM docs LIMIT 10"),
    ("Hybrid", "QUERY 'search' FROM docs LIMIT 10 USING HYBRID"),
    ("Full", "QUERY 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128, exact = true)"),
    ("CTE_Prefetch", "WITH a AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech'), b AS (QUERY 'search' USING sparse LIMIT 100)\nQUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8, b SCORE THRESHOLD 0.5) FUSION RRF"),
    ("CreateCollection", "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"),
    ("Upsert", "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"),
    ("DeleteWhere", "DELETE FROM docs WHERE category = 'archived'"),
    ("OrderBy", "QUERY ORDER BY created_at DESC FROM docs LIMIT 20 WHERE status = 'active'"),
    ("WithPayload", "QUERY 'search' FROM docs LIMIT 10 WITH PAYLOAD (include = ['title', 'body']) WITH VECTOR ('dense')"),
];

fn main() {
    let iterations = 100_000;

    println!("Rust qql-rs parse only  |  {} iterations each\n", iterations);
    println!("{:<20} {:>10} {:>12}", "Query", "ns/op", "ops/s");
    println!("{}", "-".repeat(46));

    for (name, query) in QUERIES {
        // Warmup
        for _ in 0..1000 {
            qql_core::parser::Parser::parse(query).unwrap();
        }

        let start = Instant::now();
        for _ in 0..iterations {
            qql_core::parser::Parser::parse(query).unwrap();
        }
        let elapsed = start.elapsed();

        let ns_per_op = elapsed.as_nanos() as f64 / iterations as f64;
        let ops_per_sec = (iterations as f64 / elapsed.as_secs_f64()) as u64;

        println!("{:<20} {:>10.0} {:>12}", name, ns_per_op, ops_per_sec);
    }
}
