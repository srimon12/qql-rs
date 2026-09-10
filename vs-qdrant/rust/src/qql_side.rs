//! QQL runtime (`qql` 0.4.0, gRPC backend) scenario implementations —
//! symmetric with `official.rs`.
//!
//! Notes:
//!   * `USING sparse` gets native in-process BM25 via `qql::sparse` — the
//!     query side never needs precomputed sparse vectors (showcased in
//!     `query_sparse_native`).
//!   * Bulk ingest is one `upsert_many` call per collection — point dicts
//!     in, no batch loop, no re-parse, schema fetched once.
//!   * Vectors use `Value::F32Array` zero-copy transfer (no per-element float boxing).
//!   * Result extraction uses typed `ExecutionReport` helpers (count, facet, ids).

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use qql::executor::{Executor, OnError, PreparedStatement};
use qql::sparse;
use qql::Value;

pub const BATCH_BERLIN: usize = 100;
pub const BATCH_LEGAL: usize = 32;

#[inline]
fn value_dense(vec: &[f32]) -> Value {
    Value::F32Array(vec.to_vec())
}

fn value_sparse(sv: &serde_json::Value) -> Value {
    let indices: Vec<Value> = sv["indices"].as_array().unwrap().iter()
        .map(|v| Value::Int(v.as_u64().unwrap() as i64)).collect();
    let values: Vec<f32> = sv["values"].as_array().unwrap().iter()
        .map(|v| v.as_f64().unwrap() as f32).collect();
    Value::Dict(vec![
        ("indices".into(), Value::List(indices)),
        ("values".into(), Value::F32Array(values)),
    ])
}

fn value_multivector(flat_data: &[f32], dim: usize) -> Value {
    Value::Dict(vec![
        ("data".into(), Value::F32Array(flat_data.to_vec())),
        ("dim".into(), Value::Int(dim as i64)),
    ])
}

fn value_multivector_rows(flat_rows: &[Vec<f32>]) -> Value {
    let flat: Vec<f32> = flat_rows.iter().flat_map(|r| r.iter().copied()).collect();
    let dim = flat_rows.first().map(|r| r.len()).unwrap_or(128);
    value_multivector(&flat, dim)
}

/// Facet values are compared as their wire text: keyword verbatim, numbers and
/// bools in canonical decimal form. District is keyword-only today; the other
/// arms keep the comparison total without falling back to JSON.
fn facet_value_string(value: qql_plan::PlanFacetValue) -> String {
    match value {
        qql_plan::PlanFacetValue::Keyword(s) => s,
        qql_plan::PlanFacetValue::Integer(i) => i.to_string(),
        qql_plan::PlanFacetValue::Bool(b) => b.to_string(),
    }
}

/// Consume a single-statement report into its typed hits **without cloning**
/// (the official SDK also moves its result vector out of the call). This keeps
/// the timed path free of harness-side copies.
fn report_hits(report: qql::executor::ExecutionReport) -> Vec<qql::executor::SearchHit> {
    take_hits(report.results.into_iter().next().and_then(|r| r.data))
}

/// Typed hits out of response data, moving (never cloning) the hit vector.
fn take_hits(data: Option<qql::executor::ExecData>) -> Vec<qql::executor::SearchHit> {
    match data {
        Some(qql::executor::ExecData::Hits(hits)) => hits,
        _ => Vec::new(),
    }
}

pub struct QqlScenarios {
    exec: Executor,
    prepared_dense: tokio::sync::OnceCell<PreparedStatement>,
}

impl QqlScenarios {
    /// gRPC transport — parity with the official Rust client's transport.
    pub async fn new(url_grpc: &str) -> Result<Self> {
        Ok(Self {
            exec: Executor::grpc(url_grpc, None)?,
            prepared_dense: tokio::sync::OnceCell::new(),
        })
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
        self.exec.execute(&format!("CREATE INDEX ON COLLECTION {name} FOR district TYPE keyword"), OnError::Stop).await?;
        self.exec.execute(&format!("CREATE INDEX ON COLLECTION {name} FOR price TYPE float"), OnError::Stop).await?;
        Ok(())
    }

    pub async fn create_legal(&self, name: &str) -> Result<()> {
        self.exec.execute(&format!(r#"
            CREATE COLLECTION {name} (
                dense VECTOR(384, COSINE),
                bm25 SPARSE,
                colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0)
            )"#), OnError::Stop).await?;
        self.exec.execute(&format!("CREATE INDEX ON COLLECTION {name} FOR court TYPE keyword"), OnError::Stop).await?;
        self.exec.execute(&format!("CREATE INDEX ON COLLECTION {name} FOR year TYPE integer"), OnError::Stop).await?;
        Ok(())
    }

    // ----------------------------------------------------------- ingest ----
    pub async fn ingest_berlin(
        &self, name: &str, docs: &[serde_json::Value],
        dense: &[f32], sparse: &[serde_json::Value],
    ) -> Result<f64> {
        let t0 = Instant::now();
        let mut rows = Vec::with_capacity(docs.len());
        for (i, doc) in docs.iter().enumerate() {
            let mut pt = qql_core::ast::Value::from_json(doc.clone())?;
            let d_vec = value_dense(&dense[i * 384..(i + 1) * 384]);
            let s_val = value_sparse(&sparse[i]);
            if let qql_core::ast::Value::Dict(ref mut map) = pt {
                map.push((
                    "vector".to_string(),
                    qql_core::ast::Value::Dict(vec![
                        ("dense".to_string(), d_vec),
                        ("bm25".to_string(), s_val),
                    ]),
                ));
            }
            rows.push(pt);
        }
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
        let mut rows = Vec::with_capacity(docs.len());
        for (i, doc) in docs.iter().enumerate() {
            let mut pt = qql_core::ast::Value::from_json(doc.clone())?;
            let d_vec = value_dense(&dense[i * 384..(i + 1) * 384]);
            let s_val = value_sparse(&sparse[i]);
            let c_val = value_multivector(&colbert_flat[offsets[i]..offsets[i + 1]], 128);
            if let qql_core::ast::Value::Dict(ref mut map) = pt {
                map.push((
                    "vector".to_string(),
                    qql_core::ast::Value::Dict(vec![
                        ("dense".to_string(), d_vec),
                        ("bm25".to_string(), s_val),
                        ("colbert".to_string(), c_val),
                    ]),
                ));
            }
            rows.push(pt);
        }
        self.exec.upsert_many(name, rows, BATCH_LEGAL, OnError::Stop).await?;
        Ok(t0.elapsed().as_secs_f64())
    }

    // ------------------------------------------------------------- reads ----
    pub async fn query_dense(&self, name: &str, qvec: &[f32]) -> Result<Vec<qql::executor::SearchHit>> {
        let rep = self.exec.execute_with_named_params(
            &format!("QUERY :dv FROM {name} USING dense LIMIT 10"),
            &[("dv", value_dense(qvec))],
            OnError::Stop,
        ).await?;
        Ok(report_hits(rep))
    }

    pub async fn query_dense_filtered(
        &self, name: &str, qvec: &[f32],
    ) -> Result<Vec<qql::executor::SearchHit>> {
        let rep = self.exec.execute_with_named_params(
            &format!("QUERY :dv FROM {name} USING dense WHERE price < 150.0 AND guests >= 2 LIMIT 10"),
            &[("dv", value_dense(qvec))],
            OnError::Stop,
        ).await?;
        Ok(report_hits(rep))
    }

    /// Precomputed sparse query vector (the parity baseline).
    pub async fn query_sparse(
        &self, name: &str, sv: &serde_json::Value,
    ) -> Result<Vec<qql::executor::SearchHit>> {
        let rep = self.exec.execute_with_named_params(
            &format!("QUERY :sv FROM {name} USING bm25 LIMIT 10"),
            &[("sv", value_sparse(sv))],
            OnError::Stop,
        ).await?;
        Ok(report_hits(rep))
    }

    /// Native in-process BM25: the qql runtime embeds the query TEXT locally
    /// (no HTTP embedder, no precomputed vectors) — the zero-dependency
    /// sparse path the official SDKs have no equivalent for.
    pub async fn query_sparse_native(
        &self, name: &str, text: &str,
    ) -> Result<(Vec<qql::executor::SearchHit>, sparse::SparseVector)> {
        let sv = sparse::embed_query(text);
        let rep = self.exec.execute_with_named_params(
            &format!("QUERY :sv FROM {name} USING bm25 LIMIT 10"),
            &[("sv", Value::Dict(vec![
                ("indices".into(), Value::List(sv.indices.iter().map(|&i| Value::Int(i as i64)).collect())),
                ("values".into(), Value::F32Array(sv.values.clone())),
            ]))],
            OnError::Stop,
        ).await?;
        Ok((report_hits(rep), sv))
    }

    pub async fn query_hybrid(
        &self, name: &str, qvec: &[f32], sv: &serde_json::Value,
    ) -> Result<Vec<qql::executor::SearchHit>> {
        let rep = self.exec.execute_with_named_params(
            &format!(
                "WITH d AS (QUERY :dv FROM {name} USING dense PARAMS (hnsw_ef = 128) LIMIT 50), \
                       s AS (QUERY :sv FROM {name} USING bm25 LIMIT 50) \
                 QUERY FUSION RRF FROM {name} PREFETCH (d, s) LIMIT 10"),
            &[("dv", value_dense(qvec)), ("sv", value_sparse(sv))],
            OnError::Stop,
        ).await?;
        Ok(report_hits(rep))
    }

    pub async fn query_colbert(
        &self, name: &str, flat_rows: &[Vec<f32>],
    ) -> Result<Vec<qql::executor::SearchHit>> {
        let rep = self.exec.execute_with_named_params(
            &format!("QUERY :mv FROM {name} USING colbert LIMIT 10"),
            &[("mv", value_multivector_rows(flat_rows))],
            OnError::Stop,
        ).await?;
        Ok(report_hits(rep))
    }

    pub async fn scroll_pages(
        &self, name: &str, pages: usize, batch: u64,
    ) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        let mut offset: Option<u64> = None;
        for _ in 0..pages {
            let sql = match offset {
                Some(o) => format!("SCROLL FROM {name} AFTER {o} LIMIT {batch}"),
                None => format!("SCROLL FROM {name} LIMIT {batch}"),
            };
            let rep = self.exec.execute(&sql, OnError::Stop).await?;
            let page_ids = rep.ids(0);
            if page_ids.is_empty() {
                break;
            }
            if let Some(&last) = page_ids.last() {
                offset = Some(last);
            }
            ids.extend(page_ids);
        }
        Ok(ids)
    }

    pub async fn count_berlin(&self, name: &str) -> Result<u64> {
        let report = self.exec.execute(&format!("COUNT FROM {name} WHERE price < 150.0"), OnError::Stop).await?;
        Ok(report.first_count().unwrap_or(0))
    }

    pub async fn count_legal(&self, name: &str) -> Result<u64> {
        let report = self.exec.execute(&format!("COUNT FROM {name} WHERE year >= 2010"), OnError::Stop).await?;
        Ok(report.first_count().unwrap_or(0))
    }

    pub async fn facet_district(
        &self, name: &str,
    ) -> Result<Vec<(String, u64)>> {
        let report = self.exec.execute(&format!("FACET district FROM {name} LIMIT 20 EXACT true"), OnError::Stop).await?;
        Ok(report
            .first_facet()
            .unwrap_or_default()
            .into_iter()
            .map(|(value, count)| (facet_value_string(value), count))
            .collect())
    }

    // ----------------------------------------------------------- writes ----
    pub async fn update_payload(&self, name: &str) -> Result<()> {
        self.exec.execute(&format!("UPDATE {name} SET PAYLOAD = {{rating: 4.5}} WHERE district = 'Mitte' WAIT true"), OnError::Stop).await?;
        Ok(())
    }

    pub async fn delete_by_filter(&self, name: &str) -> Result<()> {
        self.exec.execute(&format!("DELETE FROM {name} WHERE price > 250.0 WAIT true"), OnError::Stop).await?;
        Ok(())
    }

    pub async fn prepared_rerun(
        &self, name: &str, qvecs: &[Vec<f32>],
    ) -> Result<Vec<qql::executor::SearchHit>> {
        let prepared = self.prepared_dense.get_or_try_init(|| async {
            let p = self.exec.prepare(&format!("QUERY :dv FROM {name} USING dense LIMIT 10")).await?;
            anyhow::ensure!(p.is_planned(), "dense query template should pre-plan (vector-only placeholders)");
            Ok(p)
        }).await?;

        let mut hits = Vec::new();
        for v in qvecs {
            let resp = self.exec.execute_prepared(
                prepared,
                &HashMap::from([("dv".to_string(), Value::F32Array(v.clone()))]),
            ).await?;
            hits = take_hits(resp.data);
        }
        Ok(hits)
    }
}
