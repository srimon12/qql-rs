//! Mock-executor E2E: parse → prepare → plan → mock dispatch.
//!
//! Uses the same `bench/queries.json` corpus as `parse`/`explain` so the three
//! suites stay comparable. Two corpus entries carry an `e2e` counterpart (the
//! TEXT+MODEL+USING form): bare `QUERY '…'` has no vector target and cannot
//! resolve against the mock dense+sparse topology (`QQL-MISSING-USING`).
//! Queries carrying `params` (the `Bound` query) are bound once up front —
//! bind cost itself is measured in `bench_bind` — so this suite measures the
//! execute path only. No network, no model inference.
//!
//! usage: `e2e [--iterations N] [--reps N] [--filter SUBSTR] [--json]`

#[path = "../common.rs"]
mod common;

use async_trait::async_trait;
use common::{
    Row, apply_filter, load_queries, median, parse_args, print_header, print_json, print_row,
};
use qql::client::*;
use qql::executor::{Executor, OnError};
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};
use std::hint::black_box;
use std::time::Instant;

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
        Ok(CollectionInfo {
            schema: qql::backend::CollectionSchema {
                dense_vectors: vec!["dense".to_string()],
                sparse_vectors: vec![qql::backend::SparseVectorSpec {
                    name: "sparse".to_string(),
                    index: None,
                    modifier: Some("idf".to_string()),
                }],
                ..Default::default()
            },
            ..Default::default()
        })
    }
    async fn create_collection(
        &self,
        _collection_name: &str,
        _req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn update_collection(
        &self,
        _collection_name: &str,
        _req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn create_field_index(
        &self,
        _collection_name: &str,
        _req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_field_index(
        &self,
        _collection: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn execute_planned(
        &self,
        _op: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, QqlError> {
        Ok(serde_json::json!({"result": [], "status": "ok", "time": 0.0}))
    }
    async fn execute_query_batch(
        &self,
        _collection: &str,
        _batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        Ok(vec![])
    }
    async fn execute_update_batch(
        &self,
        _collection: &str,
        _batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        Ok(vec![])
    }
}

/// Bind `params` once so the timed loop measures execute only. Prefers the
/// query's `e2e` counterpart when the corpus provides one (see queries.json).
fn bound_query(query: &common::BenchQuery) -> String {
    let qql = query.e2e.as_deref().unwrap_or(&query.qql);
    match &query.params {
        Some(p) => qql_core::params_json::bind_str_with_params(qql, p, false)
            .expect("benchmark params must bind"),
        None => qql.to_string(),
    }
}

async fn bench_query(executor: &Executor, qql: &str, warmup: usize, iterations: usize) -> f64 {
    for _ in 0..warmup {
        black_box(
            executor
                .execute(qql, OnError::Stop)
                .await
                .expect("benchmark query must execute"),
        );
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(
            executor
                .execute(qql, OnError::Stop)
                .await
                .expect("benchmark query must execute"),
        );
    }
    start.elapsed().as_nanos() as f64 / iterations as f64
}

#[tokio::main]
async fn main() {
    let args = parse_args("e2e", 50_000, 3);
    let queries = apply_filter(load_queries(), args.filter.as_deref());
    // Bind up front (see module docs); fail fast on a bad corpus entry.
    let bound: Vec<(String, String)> = queries
        .iter()
        .map(|q| (q.name.clone(), bound_query(q)))
        .collect();
    // Smoke the bound queries once so a corpus regression fails before timing.
    let executor = Executor::new(Box::new(MockQdrant), None);
    for (name, qql) in &bound {
        executor
            .execute(qql, OnError::Stop)
            .await
            .unwrap_or_else(|e| panic!("e2e corpus query '{name}' must execute: {e}"));
    }

    if !args.json {
        print_header("Rust qql-runtime E2E (mock dispatch)", &args);
    }
    let mut rows = Vec::with_capacity(bound.len());
    for (name, qql) in &bound {
        let mut samples = Vec::with_capacity(args.reps);
        for _ in 0..args.reps {
            samples.push(bench_query(&executor, qql, 100, args.iterations).await);
        }
        rows.push(Row {
            name: name.clone(),
            ns_per_op: median(samples),
        });
        if !args.json {
            print_row(rows.last().expect("row just pushed"));
        }
    }
    if args.json {
        print_json("e2e", &args, &rows);
    }
}
