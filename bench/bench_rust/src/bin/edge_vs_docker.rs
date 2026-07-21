//! Quick bench: qql-edge (in-process qdrant-edge) vs qql-docker (REST).
//!
//! Run:
//!   docker compose up -d                            # start Qdrant
//!   cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin edge_vs_docker

use std::sync::Arc;
use std::time::{Duration, Instant};

use qql::embedder::Embedder;
use qql::executor::Executor;
use qql_core::error::QqlError;

const Q_CREATE: &str = "CREATE COLLECTION bench HYBRID";
const Q_DROP: &str = "DROP COLLECTION bench";
const Q_SEARCH: &str = "QUERY 'search' FROM bench LIMIT 10";
const Q_HYBRID: &str = "QUERY 'search' FROM bench LIMIT 10 USING HYBRID";
const Q_UPSERT: &str = "UPSERT INTO bench VALUES {id: 1, text: 'hello', tag: 'db'}";

const ALL_QUERIES: &[(&str, &str)] = &[
    ("SimpleSearch", Q_SEARCH),
    ("HybridSearch", Q_HYBRID),
    ("Upsert", Q_UPSERT),
];

struct NoopEmbedder;

#[async_trait::async_trait]
impl Embedder for NoopEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(vec![0.0f32; 384])
    }
    async fn embed_sparse(&self, _text: &str) -> Result<qql::sparse::SparseVector, QqlError> {
        Ok(qql::sparse::SparseVector { indices: vec![0, 1], values: vec![1.0, 0.5] })
    }
    async fn embed_dense_batch(&self, texts: &[String], _model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        Ok(vec![vec![0.0f32; 384]; texts.len()])
    }
}

fn ns_per_op(dur: Duration, n: usize) -> f64 { dur.as_nanos() as f64 / n as f64 }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iters = 500;

    // ── Edge ─────────────────────────────────────────────────────
    let edge_tmp = tempfile::TempDir::new()?;
    let edge_qdrant = Box::new(qql_edge::EdgeQdrant::new(edge_tmp.path(), false));
    let edge_emb = Some(Arc::new(NoopEmbedder) as Arc<dyn Embedder>);
    let edge_ex = Executor::with_embedder(edge_qdrant, None, edge_emb);
    edge_ex.execute(Q_CREATE).await?;
    println!("Edge: warming up...");
    for (_, q) in ALL_QUERIES { for _ in 0..10 { edge_ex.execute(q).await?; } }
    println!("Edge: benchmarking ({} iters)...", iters);

    let mut edge_ns = Vec::new();
    let mut edge_labels = Vec::new();
    for (label, q) in ALL_QUERIES {
        let start = Instant::now();
        for _ in 0..iters { edge_ex.execute(q).await?; }
        let dur = start.elapsed();
        edge_ns.push(ns_per_op(dur, iters));
        edge_labels.push(*label);
    }

    // ── Docker ───────────────────────────────────────────────────
    let rest = Box::new(qql::rest::RestQdrant::new("http://localhost:6333", None)?);
    let emb = Some(Arc::new(NoopEmbedder) as Arc<dyn Embedder>);
    let docker_ex = Executor::with_embedder(rest, None, emb);
    let _ = docker_ex.execute(Q_DROP).await;
    docker_ex.execute(Q_CREATE).await?;
    println!("Docker: warming up...");
    for (_, q) in ALL_QUERIES { for _ in 0..10 { docker_ex.execute(q).await?; } }
    println!("Docker: benchmarking ({} iters)...", iters);

    let mut docker_ns = Vec::new();
    for (_label, q) in ALL_QUERIES {
        let start = Instant::now();
        for _ in 0..iters { docker_ex.execute(q).await?; }
        let dur = start.elapsed();
        docker_ns.push(ns_per_op(dur, iters));
    }

    // ── Results ──────────────────────────────────────────────────
    println!("\n═══════════════════════════════════════════════════════");
    println!("  Backend Comparison  ({} iters each)", iters);
    println!("=======================================================");
    println!("{:<16} {:>10} {:>10} {:>10}", "Query", "Edge", "Docker", "Speedup");
    println!("{:<16} {:>10} {:>10} {:>10}", "", "ns/op", "ns/op", "Docker/Edge");
    println!("-------------------------------------------------------");
    for i in 0..ALL_QUERIES.len() {
        let ratio = docker_ns[i] / edge_ns[i];
        let winner = if ratio > 1.0 { "EDGE" } else { "DOCK" };
        println!("{:<16} {:>8.0} {:>8.0} {:>8.2}x {}",
            edge_labels[i], edge_ns[i], docker_ns[i], ratio, winner);
    }
    println!("-------------------------------------------------------");
    println!("  Edge  = qdrant-edge (in-process)");
    println!("  Docker= Qdrant REST (localhost:6333)");
    println!("═══════════════════════════════════════════════════════\n");

    Ok(())
}
