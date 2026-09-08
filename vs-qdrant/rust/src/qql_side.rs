//! QQL runtime (`qql` 0.4.0, gRPC backend) scenario implementations —
//! symmetric with `official.rs`.
//!
//! Notes:
//!   * `USING sparse` gets native in-process BM25 via `qql::sparse` — the
//!     query side never needs precomputed sparse vectors (showcased in
//!     `query_sparse_native`).
//!   * Bulk ingest is one `upsert_many` call per collection — point dicts
//!     in, no batch loop, no re-parse, schema fetched once.

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use qql::executor::{ExecResponse, Executor, OnError};
use qql::sparse;
use qql::Value;
use qql_core::params::bind_stmt;

pub const BATCH_BERLIN: usize = 100;
pub const BATCH_LEGAL: usize = 32;

fn value_f32(v: f32) -> Value {
    Value::Float(v as f64)
}

fn value_dense(vec: &[f32]) -> Value {
    Value::List(vec.iter().map(|&f| value_f32(f)).collect())
}

fn value_sparse(sv: &serde_json::Value) -> Value {
    Value::Dict(vec![
        (
            "indices".into(),
            Value::List(sv["indices"].as_array().unwrap()
                .iter().map(|v| Value::Int(v.as_u64().unwrap() as i64)).collect()),
        ),
        (
            "values".into(),
            Value::List(sv["values"].as_array().unwrap()
                .iter().map(|v| value_f32(v.as_f64().unwrap() as f32)).collect()),
        ),
    ])
}

fn value_multivector(flat_rows: &[Vec<f32>]) -> Value {
    Value::List(flat_rows.iter().map(|r| value_dense(r)).collect())
}

/// Extract the hits array (`[{id, score, payload…}]`) from one response.
pub fn hits_of(resp: &ExecResponse) -> Vec<serde_json::Value> {
    resp.data.as_ref()
        .and_then(|d| d.as_array().cloned())
        .unwrap_or_default()
}

pub struct QqlScenarios {
    exec: Executor,
}

impl QqlScenarios {
    /// gRPC transport — parity with the official Rust client's transport.
    pub async fn new(url_grpc: &str) -> Result<Self> {
        Ok(Self { exec: Executor::grpc(url_grpc, None)? })
    }

    /// Parse → AST-level param bind → execute one statement (the same path
    /// the pyqql/nqql SDKs take; textual re-parse of bound sparse/multivector
    /// object literals is not supported by the grammar).
    async fn exec_bound(
        &self, sql: &str, values: Vec<(&str, Value)>,
    ) -> Result<ExecResponse> {
        let mut stmt = qql::Parser::parse(sql)?;
        let lookup = |name: &str| {
            values.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone())
        };
        bind_stmt(&mut stmt, lookup, &[])?;
        Ok(self.exec.execute_node(stmt).await?)
    }

    // ------------------------------------------------------------ setup ----
    pub async fn drop_collection(&self, name: &str) -> Result<()> {
        let _ = self.exec.execute(&format!("DROP COLLECTION {name}"), OnError::Stop).await;
        Ok(())
    }

    pub async fn create_berlin(&self, name: &str) -> Result<()> {
        self.exec.execute(&format!(r#"
            CREATE COLLECTION {name} (
                dense VECTOR(384, COSINE),
                bm25 SPARSE
            )"#), OnError::Stop).await?;
        self.exec.execute(&format!(
            "CREATE INDEX ON COLLECTION {name} FOR district TYPE keyword"), OnError::Stop).await?;
        self.exec.execute(&format!(
            "CREATE INDEX ON COLLECTION {name} FOR price TYPE float"), OnError::Stop).await?;
        Ok(())
    }

    pub async fn create_legal(&self, name: &str) -> Result<()> {
        self.exec.execute(&format!(r#"
            CREATE COLLECTION {name} (
                dense VECTOR(384, COSINE),
                bm25 SPARSE,
                colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0)
            )"#), OnError::Stop).await?;
        self.exec.execute(&format!(
            "CREATE INDEX ON COLLECTION {name} FOR court TYPE keyword"), OnError::Stop).await?;
        self.exec.execute(&format!(
            "CREATE INDEX ON COLLECTION {name} FOR year TYPE integer"), OnError::Stop).await?;
        Ok(())
    }

    // ----------------------------------------------------------- ingest ----
    pub async fn ingest_berlin(
        &self, name: &str, docs: &[serde_json::Value],
        dense: &[f32], sparse: &[serde_json::Value],
    ) -> Result<f64> {
        let t0 = Instant::now();
        let rows: Vec<Value> = (0..docs.len())
            .map(|i| {
                let mut doc = qql_core::ast::Value::from_json(docs[i].clone())?;
                if let Value::Dict(items) = &mut doc {
                    items.push(("vector".into(), Value::Dict(vec![
                        ("dense".into(), value_dense(&dense[i * 384..(i + 1) * 384])),
                        ("bm25".into(), value_sparse(&sparse[i])),
                    ])));
                }
                Ok(doc)
            })
            .collect::<Result<Vec<_>>>()?;
        self.exec.upsert_many(name, rows, BATCH_BERLIN, OnError::Stop).await?;
        Ok(t0.elapsed().as_secs_f64())
    }

    pub async fn ingest_legal(
        &self, name: &str, docs: &[serde_json::Value],
        dense: &[f32], sparse: &[serde_json::Value],
        colbert_flat: &[f32], colbert_lens: &[usize],
    ) -> Result<f64> {
        let mut offsets = Vec::with_capacity(colbert_lens.len() + 1);
        offsets.push(0usize);
        for l in colbert_lens {
            offsets.push(offsets.last().unwrap() + l * 128);
        }
        let t0 = Instant::now();
        let rows: Vec<Value> = (0..docs.len())
            .map(|i| {
                let (o0, o1) = (offsets[i], offsets[i + 1]);
                // reshape the flat token store into per-token rows
                let mv: Vec<Vec<f32>> = colbert_flat[o0..o1]
                    .chunks(128).map(|c| c.to_vec()).collect();
                let mut doc = qql_core::ast::Value::from_json(docs[i].clone())?;
                if let Value::Dict(items) = &mut doc {
                    items.push(("vector".into(), Value::Dict(vec![
                        ("dense".into(), value_dense(&dense[i * 384..(i + 1) * 384])),
                        ("bm25".into(), value_sparse(&sparse[i])),
                        ("colbert".into(), value_multivector(&mv)),
                    ])));
                }
                Ok(doc)
            })
            .collect::<Result<Vec<_>>>()?;
        self.exec.upsert_many(name, rows, BATCH_LEGAL, OnError::Stop).await?;
        Ok(t0.elapsed().as_secs_f64())
    }

    // ------------------------------------------------------------- reads ----
    pub async fn query_dense(&self, name: &str, qvec: &[f32]) -> Result<Vec<serde_json::Value>> {
        let mut params = HashMap::new();
        params.insert("dv".to_string(), value_dense(qvec));
        let report = self
            .exec
            .execute_with_params(
                &format!("QUERY :dv FROM {name} USING dense LIMIT 10"),
                &params, OnError::Stop,
            )
            .await?;
        Ok(hits_of(&report.results[0]))
    }

    pub async fn query_dense_filtered(
        &self, name: &str, qvec: &[f32],
    ) -> Result<Vec<serde_json::Value>> {
        let mut params = HashMap::new();
        params.insert("dv".to_string(), value_dense(qvec));
        let report = self
            .exec
            .execute_with_params(
                &format!(
                    "QUERY :dv FROM {name} USING dense WHERE price < 150.0 AND guests >= 2 LIMIT 10"),
                &params, OnError::Stop,
            )
            .await?;
        Ok(hits_of(&report.results[0]))
    }

    /// Precomputed sparse query vector (the parity baseline).
    pub async fn query_sparse(
        &self, name: &str, sv: &serde_json::Value,
    ) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .exec_bound(&format!("QUERY :sv FROM {name} USING bm25 LIMIT 10"),
                        vec![("sv", value_sparse(sv))])
            .await?;
        Ok(hits_of(&resp))
    }

    /// Native in-process BM25: the qql runtime embeds the query TEXT locally
    /// (no HTTP embedder, no precomputed vectors) — the zero-dependency
    /// sparse path the official SDKs have no equivalent for.
    pub async fn query_sparse_native(
        &self, name: &str, text: &str,
    ) -> Result<(Vec<serde_json::Value>, sparse::SparseVector)> {
        let sv = sparse::embed_query(text); // murmur3 token ids, wire-compatible with Qdrant/bm25
        let resp = self
            .exec_bound(&format!("QUERY :sv FROM {name} USING bm25 LIMIT 10"),
                        vec![("sv", Value::Dict(vec![
                            ("indices".into(), Value::List(
                                sv.indices.iter().map(|&i| Value::Int(i as i64)).collect())),
                            ("values".into(), Value::List(
                                sv.values.iter().map(|&f| value_f32(f)).collect())),
                        ]))])
            .await?;
        Ok((hits_of(&resp), sv))
    }

    pub async fn query_hybrid(
        &self, name: &str, qvec: &[f32], sv: &serde_json::Value,
    ) -> Result<Vec<serde_json::Value>> {
        // hnsw_ef=128 on the dense CTE: fused rankings must be deterministic
        // across the two independently built collections.
        let resp = self
            .exec_bound(
                &format!(
                    "WITH d AS (QUERY :dv FROM {name} USING dense PARAMS (hnsw_ef = 128) LIMIT 50), \
                           s AS (QUERY :sv FROM {name} USING bm25 LIMIT 50) \
                     QUERY FUSION RRF FROM {name} PREFETCH (d, s) LIMIT 10"),
                vec![("dv", value_dense(qvec)), ("sv", value_sparse(sv))])
            .await?;
        Ok(hits_of(&resp))
    }

    pub async fn query_colbert(
        &self, name: &str, flat_rows: &[Vec<f32>],
    ) -> Result<Vec<serde_json::Value>> {
        let resp = self
            .exec_bound(&format!("QUERY :mv FROM {name} USING colbert LIMIT 10"),
                        vec![("mv", value_multivector(flat_rows))])
            .await?;
        Ok(hits_of(&resp))
    }

    pub async fn scroll_pages(
        &self, name: &str, pages: usize, batch: u64,
    ) -> Result<Vec<u64>> {
        let mut off = 0u64;
        let mut all: Vec<u64> = Vec::new();
        for _ in 0..pages {
            let resp = self
                .exec_bound(&format!("SCROLL FROM {name} AFTER :off LIMIT {batch}"),
                            vec![("off", Value::Int(off as i64))])
                .await?;
            let hits = hits_of(&resp);
            if hits.is_empty() {
                break;
            }
            for h in &hits {
                all.push(h["id"].as_str().and_then(|s| s.parse().ok())
                    .or_else(|| h["id"].as_u64()).unwrap_or(0));
            }
            // QQL `AFTER` is exclusive — resume at the boundary point.
            off = *all.last().unwrap();
        }
        Ok(all)
    }

    pub async fn count_berlin(&self, name: &str) -> Result<u64> {
        let report = self
            .exec
            .execute(&format!("COUNT FROM {name} WHERE price < 150.0"), OnError::Stop)
            .await?;
        Ok(report.results[0].data.as_ref().unwrap()["result"]["count"].as_u64().unwrap())
    }

    pub async fn count_legal(&self, name: &str) -> Result<u64> {
        let report = self
            .exec
            .execute(&format!("COUNT FROM {name} WHERE year >= 2010"), OnError::Stop)
            .await?;
        Ok(report.results[0].data.as_ref().unwrap()["result"]["count"].as_u64().unwrap())
    }

    pub async fn facet_district(
        &self, name: &str,
    ) -> Result<Vec<(serde_json::Value, u64)>> {
        let report = self
            .exec
            .execute(&format!("FACET district FROM {name} LIMIT 20 EXACT true"), OnError::Stop)
            .await?;
        Ok(report.results[0].data.as_ref().unwrap()
            .as_array().unwrap()
            .iter()
            .map(|h| (h["value"].clone(), h["count"].as_u64().unwrap()))
            .collect())
    }

    // ----------------------------------------------------------- writes ----
    pub async fn update_payload(&self, name: &str) -> Result<()> {
        self.exec
            .execute(
                &format!(
                    "UPDATE {name} SET PAYLOAD = {{rating: 4.5}} WHERE district = 'Mitte' WAIT true"),
                OnError::Stop,
            )
            .await?;
        Ok(())
    }

    pub async fn delete_by_filter(&self, name: &str) -> Result<()> {
        self.exec
            .execute(&format!("DELETE FROM {name} WHERE price > 250.0 WAIT true"), OnError::Stop)
            .await?;
        Ok(())
    }

    pub async fn prepared_rerun(
        &self, name: &str, qvecs: &[Vec<f32>],
    ) -> Result<Vec<serde_json::Value>> {
        // Prepare once (plan_template pre-lowers the vector template), then
        // per call only substitute vectors into the typed IR — no re-parse,
        // no re-plan.
        let prepared = self
            .exec
            .prepare(&format!("QUERY :dv FROM {name} USING dense LIMIT 10"))
            .await?;
        anyhow::ensure!(
            prepared.is_planned(),
            "dense query template should pre-plan (vector-only placeholders)"
        );
        let mut hits = Vec::new();
        for v in qvecs {
            let resp = self
                .exec
                .execute_prepared(&prepared, &HashMap::from([("dv".to_string(), value_dense(v))]))
                .await?;
            hits = hits_of(&resp);
        }
        Ok(hits)
    }
}
