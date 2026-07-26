use qql::executor::Executor;
use std::time::{Duration, Instant};

const QUERIES: &[(&str, &str)] = &[
    ("Simple", "QUERY 'search' FROM docs LIMIT 10"),
    ("Hybrid", "QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10"),
    ("Full", "QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2"),
    ("CTE_Prefetch", "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10"),
    ("CreateCollection", "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"),
    ("Upsert", "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"),
    ("DeleteWhere", "DELETE FROM docs WHERE category = 'archived'"),
    ("OrderBy", "QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20"),
    ("WithPayload", "QUERY 'search' FROM docs WITH PAYLOAD INCLUDE (title, body) WITH VECTOR (dense) LIMIT 10"),
];

fn bench(_name: &str, q: &str, iterations: usize) -> Duration {
    // warmup
    for _ in 0..100 {
        let _ = Executor::explain(q);
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = Executor::explain(q);
    }
    start.elapsed()
}

fn main() {
    let iterations = 100_000;
    println!("Rust qql-runtime Pure Sync Compile (explain)  |  {} iterations each\n", iterations);
    println!("{:<20} {:>12} {:>12}", "Query", "ns/op", "ops/s");
    println!("{}", "-".repeat(46));

    for (name, q) in QUERIES {
        let dur = bench(name, q, iterations);
        let ns = dur.as_nanos() as f64 / iterations as f64;
        let ops = 1_000_000_000.0 / ns;
        println!("{:<20} {:>10.0} {:>12.0}", name, ns, ops);
    }
}
