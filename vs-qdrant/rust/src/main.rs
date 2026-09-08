#![allow(non_snake_case)] // R / S mirror the report schema in the python/node legs
//! Head-to-head harness: official `qdrant-client` (gRPC) vs `qql` runtime
//! (gRPC). Same design as python/bench.py and node/bench.js — identical
//! scenarios, parity-checked, plus a native-BM25 wire-parity proof.
//!
//! Usage: cargo run --release [-- --reps 5 --iters 25]
//! Writes: ../results/rust.json

mod official;
mod qql_side;

use std::collections::HashMap;

use anyhow::Result;
use qdrant_client::qdrant::point_id::PointIdOptions;
use serde_json::{json, Value};

const URL: &str = "http://localhost:6333";
const URL_GRPC: &str = "http://localhost:6334";
const SCROLL_PAGES: usize = 3;
const SCROLL_BATCH: u32 = 256;

// ------------------------------------------------------------------ data ----
struct Data {
    berlin: Vec<Value>,
    legal: Vec<Value>,
    dense_berlin: Vec<f32>,
    dense_legal: Vec<f32>,
    sparse_berlin: Vec<Value>,
    sparse_legal: Vec<Value>,
    colbert_flat: Vec<f32>,
    colbert_lens: Vec<usize>,
    queries: HashMap<String, Vec<Value>>,
}

fn load_data() -> Result<Data> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("data");
    let read = |name: &str| -> Result<String> { Ok(std::fs::read_to_string(dir.join(name))?) };
    let f32_vec = |name: &str| -> Result<Vec<f32>> {
        Ok(std::fs::read(dir.join(name))?
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    };
    let parse_lines = |name: &str| -> Result<Vec<Value>> {
        Ok(read(name)?.lines().filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap()).collect())
    };
    let q: HashMap<String, Value> = serde_json::from_str(&read("queries.json")?)?;
    let queries: HashMap<String, Vec<Value>> = q.iter()
        .map(|(k, v)| (k.clone(), v.as_array().unwrap().clone())).collect();
    Ok(Data {
        berlin: parse_lines("berlin.jsonl")?,
        legal: parse_lines("legal.jsonl")?,
        dense_berlin: f32_vec("berlin_dense.f32")?,
        dense_legal: f32_vec("legal_dense.f32")?,
        sparse_berlin: serde_json::from_str(&read("berlin_bm25.json")?)?,
        sparse_legal: serde_json::from_str(&read("legal_bm25.json")?)?,
        colbert_lens: serde_json::from_str(&read("legal_colbert_lens.json")?)?,
        colbert_flat: f32_vec("legal_colbert.f32")?,
        queries,
    })
}

fn f32s(v: &Value) -> Vec<f32> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
}

fn sparse_vec(v: &Value) -> official::PbSparseVector {
    official::PbSparseVector {
        values: f32s(&v["values"]),
        indices: v["indices"].as_array().unwrap().iter()
            .map(|x| x.as_u64().unwrap() as u32).collect(),
    }
}

fn colbert_offsets(lens: &[usize]) -> Vec<usize> {
    let mut o = Vec::with_capacity(lens.len() + 1);
    o.push(0);
    for l in lens {
        o.push(o.last().unwrap() + l * 128);
    }
    o
}

// --------------------------------------------------------------- timing ----
async fn timed_read<F, Fut>(mut f: F, reps: usize, iters: usize) -> Result<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut samples = Vec::new();
    for r in 0..reps {
        if r == 0 {
            f().await?; // warmup rep untimed
        }
        let t0 = std::time::Instant::now();
        for _ in 0..iters {
            f().await?;
        }
        samples.push(t0.elapsed().as_secs_f64() * 1000.0 / iters as f64);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = samples[samples.len() / 2];
    Ok(json!({
        "ops_per_sec": (1000.0 / med) as u64,
        "p50_ms": (med * 1000.0).round() / 1000.0,
        "p95_ms": (samples[samples.len() - 1] * 1000.0).round() / 1000.0,
        "reps": reps,
        "iters": iters,
    }))
}

async fn http_get_json(url: &str) -> Result<Value> {
    let url = url.to_string();
    let body = tokio::task::spawn_blocking(move || -> Result<String> {
        let resp = ureq::get(&url).call()?;
        Ok(resp.into_string()?)
    }).await??;
    Ok(serde_json::from_str(&body)?)
}

async fn wait_until_ready(collection: &str, expected: u64) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        let body = http_get_json(&format!("{URL}/collections/{collection}")).await?;
        if body["result"]["points_count"].as_u64() == Some(expected) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!("{collection} never reached {expected} points");
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

// --------------------------------------------------------------- parity ----
fn hit_id(h: &Value) -> u64 {
    h["id"].as_u64().or_else(|| h["id"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

fn compare_hits(official: &[Value], qql: &[Value]) -> Value {
    let o: Vec<u64> = official.iter().map(hit_id).collect();
    let q: Vec<u64> = qql.iter().map(hit_id).collect();
    let inter = o.iter().filter(|id| q.contains(id)).count();
    let mut all = o.clone();
    all.extend(q.iter().cloned());
    all.sort_unstable();
    all.dedup();
    let overlap = if all.is_empty() { 1.0 } else { inter as f64 / all.len() as f64 };
    let mut max_diff = 0.0f64;
    for oh in official {
        if let Some(qh) = qql.iter().find(|h| hit_id(h) == hit_id(oh)) {
            let a = oh["score"].as_f64().unwrap_or(0.0);
            let b = qh["score"].as_f64().unwrap_or(0.0);
            max_diff = max_diff.max((a - b).abs());
        }
    }
    json!({
        "top1_match": !o.is_empty() && !q.is_empty() && o[0] == q[0],
        "jaccard_overlap": (overlap * 1000.0).round() / 1000.0,
        "max_score_diff": (max_diff * 1e6).round() / 1e6,
    })
}

fn compare_exact(official: &Value, qql: &Value) -> Value {
    let ok = official == qql;
    json!({
        "match": ok,
        "detail": if ok { String::new() } else {
            format!("{official} != {qql}").chars().take(200).collect()
        },
    })
}

fn facet_map(hits: Vec<(Value, u64)>) -> Value {
    let mut m = serde_json::Map::new();
    for (v, c) in hits {
        m.insert(v.to_string(), Value::from(c));
    }
    Value::Object(m)
}

async fn rest_point(collection: &str, pid: u64) -> Result<Value> {
    let url = format!("{URL}/collections/{collection}/points");
    let body = tokio::task::spawn_blocking(move || -> Result<String> {
        let resp = ureq::post(&url)
            .send_json(serde_json::json!({"ids": [pid], "with_payload": true, "with_vector": true}))?;
        Ok(resp.into_string()?)
    }).await??;
    Ok(serde_json::from_str::<Value>(&body)?["result"][0].clone())
}

fn vectors_close(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| match (x, y) {
                    (Value::Array(_), Value::Array(_)) => vectors_close(x, y, tol),
                    _ => (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() <= tol,
                })
        }
        _ => false,
    }
}

// ------------------------------------------------------------------ main ----
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name)
        .and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
    let reps: usize = arg("--reps").unwrap_or(5);
    let iters: usize = arg("--iters").unwrap_or(25);

    let data = load_data()?;
    let official = official::OfficialScenarios::new(URL_GRPC).await?;
    let qql = qql_side::QqlScenarios::new(URL_GRPC).await?;
    let mut R = json!({"meta": meta(), "scenarios": {}, "ingest": {}, "parity": {}});

    let colls = json!({
        "berlin": {"official": "vsq_berlin_official", "qql": "vsq_berlin_qql"},
        "legal": {"official": "vsq_legal_official", "qql": "vsq_legal_qql"},
    });
    let c = |dom: &str, side: &str| colls[dom][side].as_str().unwrap().to_string();

    for side in ["official", "qql"] {
        for dom in ["berlin", "legal"] {
            match side {
                "official" => official.drop_collection(&c(dom, side)).await?,
                _ => qql.drop_collection(&c(dom, side)).await?,
            }
        }
    }
    official.create_berlin(&c("berlin", "official")).await?;
    official.create_legal(&c("legal", "official")).await?;
    qql.create_berlin(&c("berlin", "qql")).await?;
    qql.create_legal(&c("legal", "qql")).await?;

    // ingest (timed once per side)
    let t0 = std::time::Instant::now();
    official.ingest_berlin(&c("berlin", "official"), &data.berlin,
        &data.dense_berlin, &data.sparse_berlin).await?;
    official.ingest_legal(&c("legal", "official"), &data.legal,
        &data.dense_legal, &data.sparse_legal, &data.colbert_flat, &data.colbert_lens).await?;
    R["ingest"]["official_sec"] = json!((t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0);

    let t0 = std::time::Instant::now();
    qql.ingest_berlin(&c("berlin", "qql"), &data.berlin,
        &data.dense_berlin, &data.sparse_berlin).await?;
    qql.ingest_legal(&c("legal", "qql"), &data.legal,
        &data.dense_legal, &data.sparse_legal, &data.colbert_flat, &data.colbert_lens).await?;
    R["ingest"]["qql_sec"] = json!((t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0);
    R["ingest"]["points"] = json!({
        "berlin": data.berlin.len(), "legal": data.legal.len(),
        "batch": {"berlin": 100, "legal": 32},
    });
    println!("ingest: official {}s vs qql {}s", R["ingest"]["official_sec"], R["ingest"]["qql_sec"]);

    for side in ["official", "qql"] {
        for dom in ["berlin", "legal"] {
            let expected = match dom {
                "berlin" => data.berlin.len(),
                _ => data.legal.len(),
            };
            wait_until_ready(&c(dom, side), expected as u64).await?;
        }
    }

    let mut S = serde_json::Map::new();
    let bq = &data.queries["berlin"];
    let (bq0d, bq1d, bq2d) = (f32s(&bq[0]["dense"]), f32s(&bq[1]["dense"]), f32s(&bq[2]["dense"]));
    let (bq0s, bq2s) = (&bq[0]["sparse"], &bq[2]["sparse"]);
    let legal_colbert = {
        let offs = colbert_offsets(&data.colbert_lens);
        let (o0, o1) = (offs[0], offs[1]);
        let tokens = data.colbert_lens[0];
        (data.colbert_flat[o0..o1].to_vec(), tokens)
    };
    let legal_multivec: Vec<Vec<f32>> = legal_colbert.0
        .chunks(128).map(|c| c.to_vec()).collect();
    let prepared_vecs: Vec<Vec<f32>> = bq.iter().map(|q| f32s(&q["dense"])).collect();

    macro_rules! read_pair {
        ($label:expr, $o_call:expr, $q_call:expr) => {
            read_pair!(INNER, $label, $o_call, $q_call,
                |o: Vec<Value>, q: Vec<Value>| compare_hits(&o, &q))
        };
        ($label:expr, $o_call:expr, $q_call:expr, $parity:expr) => {
            read_pair!(INNER, $label, $o_call, $q_call, $parity)
        };
        (INNER, $label:expr, $o_call:expr, $q_call:expr, $parity:expr) => {{
            let official_side = timed_read(|| async { $o_call.await.map(|_| ()) }, reps, iters).await?;
            let qql_side = timed_read(|| async { $q_call.await.map(|_| ()) }, reps, iters).await?;
            let parity = $parity($o_call.await?, $q_call.await?);
            S.insert($label.into(), json!({"official": official_side, "qql": qql_side, "parity": parity}));
            let ratio = qql_side["ops_per_sec"].as_f64().unwrap()
                / official_side["ops_per_sec"].as_f64().unwrap();
            println!("{:<24} official {:>9} | qql {:>9} | x{:.2} | parity {}",
                $label, official_side["ops_per_sec"], qql_side["ops_per_sec"], ratio,
                serde_json::to_string(&parity).unwrap().chars().take(90).collect::<String>());
        }};
    }

    read_pair!("query_dense",
        official.query_dense(&c("berlin", "official"), &bq0d),
        qql.query_dense(&c("berlin", "qql"), &bq0d));
    read_pair!("query_dense_filtered",
        official.query_dense_filtered(&c("berlin", "official"), &bq1d),
        qql.query_dense_filtered(&c("berlin", "qql"), &bq1d));
    read_pair!("query_sparse",
        official.query_sparse(&c("berlin", "official"), &sparse_vec(bq0s)),
        qql.query_sparse(&c("berlin", "qql"), bq0s));
    read_pair!("query_hybrid",
        official.query_hybrid(&c("berlin", "official"), &bq2d, &sparse_vec(bq2s)),
        qql.query_hybrid(&c("berlin", "qql"), &bq2d, bq2s));

    // RRF tie-breaking baseline: the official client's own self-consistency
    // across two runs (server-side RRF ordering is nondeterministic at
    // equal-score ties) — so the qql-vs-official delta has a fair reference.
    {
        let o1 = official.query_hybrid(&c("berlin", "official"), &bq2d, &sparse_vec(bq2s)).await?;
        let o2 = official.query_hybrid(&c("berlin", "official"), &bq2d, &sparse_vec(bq2s)).await?;
        let q = qql.query_hybrid(&c("berlin", "qql"), &bq2d, bq2s).await?;
        R["parity"]["hybrid_tie_baseline"] = json!({
            "official_vs_official": compare_hits(&o1, &o2),
            "qql_vs_official": compare_hits(&o1, &q),
        });
        println!("hybrid_tie_baseline: {}",
            serde_json::to_string(&R["parity"]["hybrid_tie_baseline"]).unwrap());
    }
    read_pair!("scroll_pages",
        official.scroll_pages(&c("berlin", "official"), SCROLL_PAGES, SCROLL_BATCH),
        qql.scroll_pages(&c("berlin", "qql"), SCROLL_PAGES, SCROLL_BATCH as u64),
        |o: Vec<u64>, q: Vec<u64>| compare_hits(
            &o.iter().map(|id| json!({"id": id, "score": 0.0})).collect::<Vec<_>>(),
            &q.iter().map(|id| json!({"id": id, "score": 0.0})).collect::<Vec<_>>()));
    read_pair!("count_berlin",
        official.count_berlin(&c("berlin", "official")),
        qql.count_berlin(&c("berlin", "qql")),
        |o: u64, q: u64| compare_exact(&json!(o), &json!(q)));
    read_pair!("facet_district",
        official.facet_district(&c("berlin", "official")),
        qql.facet_district(&c("berlin", "qql")),
        |o, q| compare_exact(&facet_map(o), &facet_map(q)));
    read_pair!("query_colbert",
        official.query_colbert(&c("legal", "official"), legal_multivec.clone()),
        qql.query_colbert(&c("legal", "qql"), &legal_multivec));
    read_pair!("count_legal",
        official.count_legal(&c("legal", "official")),
        qql.count_legal(&c("legal", "qql")),
        |o: u64, q: u64| compare_exact(&json!(o), &json!(q)));
    read_pair!("prepared_rerun",
        official.prepared_rerun(&c("berlin", "official"), &prepared_vecs),
        qql.prepared_rerun(&c("berlin", "qql"), &prepared_vecs));

    // Native BM25 showcase + wire-parity proof.
    //
    // 1. Token ids: qql-embed's murmur3 ids must equal the pipeline's
    //    fastembed `Qdrant/bm25` ids as a SET (fastembed returns them in
    //    token order, qql-embed sorts — an ordering difference only).
    // 2. Document side: qql-embed's tf-saturation document weights must be
    //    byte-identical to the precomputed vectors both sides ingested.
    // 3. Query side: qql uses unit query weights; fastembed emits a uniform
    //    per-term weight (1.665…) — a uniform scale, so rankings agree.
    {
        let text = bq[0]["text"].as_str().unwrap();
        let (qql_hits, native_sv) =
            qql.query_sparse_native(&c("berlin", "qql"), text).await?;
        let pre = bq0s;
        let pre_indices: Vec<u32> = pre["indices"].as_array().unwrap()
            .iter().map(|v| v.as_u64().unwrap() as u32).collect();
        let mut sorted_native = native_sv.indices.clone();
        sorted_native.sort_unstable();
        let mut sorted_pre = pre_indices.clone();
        sorted_pre.sort_unstable();
        let token_ids_match = sorted_native == sorted_pre;

        // document-side wire parity (doc 0)
        let doc0_text = data.berlin[0]["description"].as_str().unwrap();
        let doc_sv = qql::sparse::embed_document(doc0_text);
        let pre_doc = &data.sparse_berlin[0];
        let pre_doc_map: HashMap<u32, f32> = pre_doc["indices"].as_array().unwrap().iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .zip(pre_doc["values"].as_array().unwrap().iter()
                .map(|v| v.as_f64().unwrap() as f32))
            .collect();
        let qql_doc_map: HashMap<u32, f32> = doc_sv.indices.iter().copied()
            .zip(doc_sv.values.iter().copied()).collect();
        let doc_identical = pre_doc_map == qql_doc_map;

        let official_native = official.query_sparse(&c("berlin", "official"),
            &official::PbSparseVector {
                indices: native_sv.indices.clone(), values: native_sv.values.clone(),
            }).await?;
        let official_pre = official.query_sparse(&c("berlin", "official"),
            &sparse_vec(pre)).await?;
        R["parity"]["native_bm25"] = json!({
            "query_token_ids_match": token_ids_match,
            "document_vectors_byte_identical": doc_identical,
            "qql_native_vs_official_native": compare_hits(&official_native, &qql_hits),
            "official_native_vs_official_precomputed": compare_hits(&official_native, &official_pre),
        });
        println!("native_bm25: tokens_match={} doc_identical={} parity={}",
            token_ids_match, doc_identical,
            serde_json::to_string(&R["parity"]["native_bm25"]).unwrap());
    }

    R["scenarios"] = Value::Object(S);

    // writes (last, parity-checked)
    official.update_payload(&c("berlin", "official")).await?;
    qql.update_payload(&c("berlin", "qql")).await?;
    R["parity"]["update_payload"] = compare_exact(
        &facet_map(official.facet_district(&c("berlin", "official")).await?),
        &facet_map(qql.facet_district(&c("berlin", "qql")).await?))["match"].clone();

    official.delete_by_filter(&c("berlin", "official")).await?;
    qql.delete_by_filter(&c("berlin", "qql")).await?;
    let expected = data.berlin.iter()
        .filter(|d| d["price"].as_f64().unwrap() <= 250.0).count() as u64;
    wait_until_ready(&c("berlin", "official"), expected).await?;
    wait_until_ready(&c("berlin", "qql"), expected).await?;
    R["parity"]["delete_by_filter"] = compare_exact(
        &json!(official.count_berlin(&c("berlin", "official")).await?),
        &json!(qql.count_berlin(&c("berlin", "qql")).await?))["match"].clone();

    // ingested-point parity (raw REST, both sides)
    for (dom, pid) in [("berlin", 1u64), ("legal", 1u64)] {
        let o = rest_point(&c(dom, "official"), pid).await?;
        let q = rest_point(&c(dom, "qql"), pid).await?;
        let ok = o["payload"] == q["payload"]
            && o["vector"].as_object().map(|m| m.len())
                == q["vector"].as_object().map(|m| m.len())
            && o["vector"].as_object().unwrap().iter().all(|(k, v)| {
                let w = &q["vector"][k];
                if v.is_array() { vectors_close(v, w, 1e-6) }
                else { v["indices"] == w["indices"] && vectors_close(&v["values"], &w["values"], 1e-6) }
            });
        R["parity"][format!("ingested_point_{dom}")] = json!(ok);
        if !ok {
            println!("PARITY FAIL ingested_point_{dom}");
        }
    }

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("results").join("rust.json");
    std::fs::create_dir_all(out.parent().unwrap())?;
    std::fs::write(&out, serde_json::to_string_pretty(&R)?)?;
    println!("\nwritten {}", out.display());
    Ok(())
}

fn meta() -> Value {
    json!({
        "timestamp": unix_ts(),
        "rust": rustc_version(),
        "qdrant_client": "1.19",
        "qql": "0.4.0",
        "qdrant_server": "1.19.1",
        "transport": "gRPC (both contenders)",
    })
}

fn unix_ts() -> String {
    format!("unix:{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
}

fn rustc_version() -> String {
    String::from_utf8(std::process::Command::new("rustc").arg("--version")
        .output().map(|o| o.stdout).unwrap_or_default())
        .unwrap_or_default().trim().to_string()
}

#[allow(dead_code)]
fn unused_point_helper(p: &qdrant_client::qdrant::PointId) -> u64 {
    match p.point_id_options.as_ref() {
        Some(PointIdOptions::Num(n)) => *n,
        _ => 0,
    }
}
