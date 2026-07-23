use async_trait::async_trait;
use qql::client::*;
use qql::executor::Executor;
use qql_core::error::QqlError;
use qql_plan::routing::Route;
use std::time::{Duration, Instant};

struct MockQdrant;

#[async_trait]
impl QdrantOps for MockQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(vec!["docs".to_string()])
    }
    async fn collection_exists(&self, _name: &str) -> Result<bool, QqlError> {
        Ok(true)
    }
    async fn get_collection_info(&self, _name: &str) -> Result<CollectionInfo, QqlError> {
        Ok(CollectionInfo::default())
    }
    async fn create_collection(&self, _req: CreateCollectionReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn execute_route(&self, _route: Route) -> Result<serde_json::Value, QqlError> {
        Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
    }
}

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

async fn bench(executor: &Executor, _name: &str, q: &str, iterations: usize) -> Duration {
    for _ in 0..100 {
        let _ = executor.execute(q).await;
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = executor.execute(q).await;
    }
    start.elapsed()
}

#[tokio::main]
async fn main() {
    let executor = Executor::new(Box::new(MockQdrant), None);
    let iterations = 100_000;
    println!("Rust qql-runtime E2E  |  {} iterations each\n", iterations);
    println!("{:<20} {:>12} {:>12}", "Query", "ns/op", "ops/s");
    println!("{}", "-".repeat(46));

    for (name, q) in QUERIES {
        let dur = bench(&executor, name, q, iterations).await;
        let ns = dur.as_nanos() as f64 / iterations as f64;
        let ops = 1_000_000_000.0 / ns;
        println!("{:<20} {:>10.0} {:>12.0}", name, ns, ops);
    }
}
