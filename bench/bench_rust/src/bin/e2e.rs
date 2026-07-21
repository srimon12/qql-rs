use async_trait::async_trait;
use qql::client::*;
use qql::executor::Executor;
use qql_core::error::QqlError;
use std::time::{Duration, Instant};

struct MockQdrant {
    collection_info: CollectionInfo,
}

impl MockQdrant {
    fn new() -> Self {
        let info = serde_json::from_value(serde_json::json!({
            "status": "green",
            "optimizer_status": "ok",
            "vectors_count": 0,
            "indexed_vectors_count": 0,
            "points_count": 0,
            "segments_count": 0,
            "config": {
                "params": {
                    "vectors": {
                        "dense": {
                            "size": 384,
                            "distance": "Cosine"
                        }
                    }
                },
                "hnsw_config": {
                    "m": 16,
                    "ef_construct": 100,
                    "full_scan_threshold": 10000,
                    "max_indexing_threads": 0,
                    "on_disk": false,
                    "payload_m": 16
                },
                "optimizer_config": {
                    "deleted_threshold": 0.2,
                    "vacuum_min_vector_number": 1000,
                    "default_segment_number": 0,
                    "max_segment_size": 10000,
                    "memmap_threshold": 10000,
                    "indexing_threshold": 20000,
                    "flush_interval_sec": 5,
                    "max_optimization_threads": 1
                },
                "wal_config": {
                    "wal_capacity_mb": 32,
                    "wal_segments_ahead": 0
                }
            },
            "payload_schema": {}
        })).unwrap();
        Self { collection_info: info }
    }
}

#[async_trait]
impl QdrantCoreOps for MockQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(vec!["docs".to_string()])
    }
    async fn collection_exists(&self, _name: &str) -> Result<bool, QqlError> {
        Ok(true)
    }
    async fn get_collection_info(&self, _name: &str) -> Result<CollectionInfo, QqlError> {
        Ok(self.collection_info.clone())
    }
    async fn create_collection(&self, _req: CreateCollectionReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn upsert(&self, _req: UpsertPointsReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn query(&self, _req: qql::pipeline::QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError> {
        Ok(vec![])
    }
    async fn query_groups(
        &self,
        _req: qql::pipeline::QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        Ok(vec![])
    }
    async fn delete(&self, _req: DeletePointsReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn update_vectors(&self, _req: UpdateVectorsReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn set_payload(&self, _req: SetPayloadReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn scroll(
        &self,
        _req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<qql::pipeline::PointId>), QqlError> {
        Ok((vec![], None))
    }
    async fn get(&self, _req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        Ok(vec![])
    }
}

#[async_trait]
impl QdrantAdminOps for MockQdrant {
    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn query_batch(
        &self,
        req: Vec<qql::pipeline::QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        Ok(vec![vec![]; req.len()])
    }
    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn count(&self, _req: CountPointsReq) -> Result<u64, QqlError> {
        Ok(0)
    }
}

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

async fn bench(executor: &Executor, _name: &str, q: &str, iterations: usize) -> Duration {
    // warmup
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
    let executor = Executor::new(Box::new(MockQdrant::new()), None);
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
