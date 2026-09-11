//! `qql doctor` — connection, embed probe, collection topology.

use super::edge::{IndexingState, collect_indexing_states};
#[cfg(feature = "rest")]
use super::runtime::probe_embed_dim;
use super::runtime::{
    classify_backend_failure, dense_vector_sizes, executor, resolve_embed_settings,
};

pub async fn handle_doctor(
    url: &str,
    use_edge: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let hosts = doctor_host_summary(executor.config(), use_edge);
    let ping = executor
        .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
        .await;
    let mut qdrant_ok = true;
    let mut qdrant_code: Option<String> = None;
    let mut qdrant_detail = String::new();
    let mut qdrant_class = "ok";
    if let Err(e) = &ping {
        qdrant_ok = false;
        qdrant_code = Some(e.code.to_string());
        qdrant_detail = e.to_string();
        qdrant_class = classify_backend_failure(&e.code, &e.message);
    }

    // Embed probe: sample request against EMBED_URL, report the real dim.
    let (endpoint_opt, model, expected_dim, dim_source) = resolve_embed_settings();
    let mut embed_line = String::from("embed: no EMBED_URL configured, literal vectors only");
    // Updated only by the REST embed probe; grpc-only builds keep the defaults.
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_observed: Option<usize> = None;
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_ok = true;
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_code: Option<String> = None;
    if let Some(endpoint) = endpoint_opt.as_deref() {
        #[cfg(feature = "rest")]
        {
            match probe_embed_dim(endpoint, &model, expected_dim).await {
                Ok(real) => {
                    embed_observed = Some(real);
                    if real == expected_dim {
                        embed_line = format!(
                            "embed: {endpoint} model={model} dim={real} (matches {dim_source}={expected_dim})"
                        );
                    } else {
                        embed_ok = false;
                        embed_code = Some("QQL-EMBEDDING-DIM".to_string());
                        embed_line = format!(
                            "[QQL-EMBEDDING-DIM] embed: {endpoint} model={model} returned dim={real} but {dim_source}={expected_dim}; fix: set EMBED_DIM={real}"
                        );
                    }
                }
                Err(e) => {
                    embed_ok = false;
                    embed_code = Some(e.code.to_string());
                    embed_line = format!(
                        "[{}] embed: probe of {endpoint} failed: {e}; fix: run `ollama serve`, `ollama pull {model}`, and check EMBED_URL",
                        e.code
                    );
                }
            }
        }
        #[cfg(not(feature = "rest"))]
        {
            let _ = (endpoint, expected_dim);
            embed_line =
                "embed: HTTP embedding probe needs the rest feature; rebuild with --features rest"
                    .to_string();
        }
    }

    // Collection vector size compare against the probed dim.
    let mut dim_mismatches: Vec<String> = Vec::new();
    let mut collections_note = String::new();
    let mut edge_indexing: Vec<IndexingState> = Vec::new();
    if qdrant_ok && !use_edge {
        match executor.client().list_collections().await {
            Ok(names) => {
                for name in names.iter().take(20) {
                    let Ok(info) = executor.client().get_collection_info(name).await else {
                        continue;
                    };
                    for (vec_name, size) in dense_vector_sizes(&info) {
                        if let Some(real) = embed_observed
                            && size as usize != real
                        {
                            let label = if vec_name.is_empty() {
                                "<default>".to_string()
                            } else {
                                vec_name.clone()
                            };
                            dim_mismatches.push(format!(
                                "[QQL-BACKEND-DIMENSION-MISMATCH] collection '{name}' vector '{label}' size={size} != embed dim={real}; fix: set EMBED_DIM={real} or recreate with VECTOR({real}, ...)"
                            ));
                        }
                    }
                }
                if names.is_empty() {
                    collections_note = "collections: none yet".to_string();
                } else {
                    collections_note =
                        format!("collections: {} ({})", names.len(), names.join(", "));
                }
            }
            Err(e) => {
                collections_note = format!("collections: list failed [{}]: {e}", e.code);
            }
        }
    } else if use_edge && qdrant_ok {
        match collect_indexing_states(executor.client()).await {
            Ok(states) => {
                collections_note = if states.is_empty() {
                    "collections: edge backend (local, none yet)".to_string()
                } else {
                    format!("collections: edge backend (local, {})", states.len())
                };
                edge_indexing = states;
            }
            Err(e) => {
                collections_note = format!("collections: edge list failed [{}]: {e}", e.code);
            }
        }
    } else if use_edge {
        collections_note = "collections: edge backend (local)".to_string();
    }

    let healthy = qdrant_ok && embed_ok && dim_mismatches.is_empty();
    let target = if use_edge {
        "the local edge backend".to_string()
    } else {
        format!("Qdrant at {url}")
    };

    if json {
        let message = if healthy {
            format!("Connected to {target}")
        } else if !qdrant_ok {
            match qdrant_class {
                "unreachable" => format!(
                    "Qdrant unreachable at {url} [{}]: {qdrant_detail}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                    qdrant_code.clone().unwrap_or_default()
                ),
                "auth" => format!(
                    "Qdrant auth failed at {url} [QQL-BACKEND-AUTH]: {qdrant_detail}; fix: set QDRANT_API_KEY to a valid key"
                ),
                _ => format!(
                    "Doctor failed for {target}: {qdrant_detail} {embed_line} {}",
                    dim_mismatches.join("; ")
                ),
            }
        } else if !embed_ok || !dim_mismatches.is_empty() {
            format!("{} {}", embed_line, dim_mismatches.join("; "))
        } else {
            format!("Connected to {target}")
        };
        println!(
            "{}",
            serde_json::json!({
                "ok": healthy,
                "healthy": healthy,
                "message": message,
                "hosts": hosts,
                "qdrant_ok": qdrant_ok,
                "qdrant_error_code": qdrant_code,
                "embed_endpoint": endpoint_opt,
                "embed_model": model,
                "embed_expected_dim": expected_dim,
                "embed_dim_source": dim_source,
                "embed_observed_dim": embed_observed,
                "embed_error_code": embed_code,
                "dim_mismatches": dim_mismatches,
                "collections_note": collections_note,
                "edge_indexing": edge_indexing,
            })
        );
    } else if !quiet {
        if qdrant_ok {
            println!("Connected to {target} (healthy)");
        } else {
            match qdrant_class {
                "unreachable" => println!(
                    "Qdrant unreachable at {url} [{}]: {qdrant_detail}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                    qdrant_code.clone().unwrap_or_default()
                ),
                "auth" => println!(
                    "Qdrant auth failed at {url} [QQL-BACKEND-AUTH]: {qdrant_detail}; fix: set QDRANT_API_KEY to a valid key"
                ),
                _ => println!("Failed to connect to {target}: {qdrant_detail}"),
            }
        }
        println!("{embed_line}");
        for mismatch in &dim_mismatches {
            println!("{mismatch}");
        }
        if !collections_note.is_empty() {
            println!("{collections_note}");
        }
        for state in &edge_indexing {
            if let Some(nudge) = &state.nudge {
                println!("{nudge}");
            }
        }
        print_doctor_hosts(&hosts);
    }
    executor.close().await?;
    if quiet && healthy {
        return Ok(());
    }
    if quiet && !healthy {
        let detail = if !qdrant_ok {
            qdrant_detail.clone()
        } else {
            format!("{embed_line} {}", dim_mismatches.join("; "))
        };
        return Err(format!("Doctor failed for {target}: {detail}").into());
    }
    if healthy {
        Ok(())
    } else {
        Err(format!("Doctor failed for {target}").into())
    }
}

/// Snapshot of embedding / rerank hosts for doctor UX (no model download).
fn doctor_host_summary(
    config: Option<&qql::config::QqlConfig>,
    use_edge: bool,
) -> serde_json::Value {
    let Some(cfg) = config else {
        return serde_json::json!({
            "backend": if use_edge { "edge" } else { "remote" },
            "dense": false,
            "multi": false,
            "image": false,
            "cross_rerank": false,
            "hints": ["no QqlConfig on executor — embedding hosts unknown"],
        });
    };
    let dense = cfg.embedding_model.as_ref().is_some_and(|m| !m.is_empty())
        || cfg
            .embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty())
        || use_edge;
    let multi = cfg
        .multi_embedding_model
        .as_ref()
        .is_some_and(|m| !m.is_empty())
        || cfg
            .multi_embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());
    let image = cfg
        .image_embedding_model
        .as_ref()
        .is_some_and(|m| !m.is_empty())
        || cfg
            .image_embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());
    let cross = cfg.rerank_model.as_ref().is_some_and(|m| !m.is_empty())
        || cfg
            .rerank_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());

    let mut hints = Vec::new();
    if !multi {
        hints.push(
            "ColBERT / AS MULTI / multivector RERANK needs multi_model or multi_embedding_* config",
        );
    }
    if !image {
        hints.push("IMAGE / CLIP vision needs image_model or image_embedding_* config");
    }
    if !cross {
        hints.push("CROSS RERANK needs reranker_model or rerank_endpoint / rerank_model");
    }
    if use_edge {
        hints.push("edge has no SHARD routing or shard-key DDL; ALTER COLLECTION covers global HNSW/optimizers and per-vector HNSW only");
    }

    serde_json::json!({
        "backend": if use_edge { "edge" } else { "remote" },
        "dense": dense,
        "dense_model": cfg.embedding_model,
        "multi": multi,
        "multi_model": cfg.multi_embedding_model,
        "image": image,
        "image_model": cfg.image_embedding_model,
        "cross_rerank": cross,
        "rerank_model": cfg.rerank_model,
        "hints": hints,
    })
}

fn print_doctor_hosts(hosts: &serde_json::Value) {
    println!(
        "Hosts: dense={} multi={} image={} cross_rerank={}",
        hosts
            .get("dense")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("multi")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("cross_rerank")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );
    if let Some(m) = hosts.get("dense_model").and_then(|v| v.as_str()) {
        println!("  dense_model: {m}");
    }
    if let Some(m) = hosts.get("multi_model").and_then(|v| v.as_str()) {
        println!("  multi_model: {m}");
    }
    if let Some(m) = hosts.get("image_model").and_then(|v| v.as_str()) {
        println!("  image_model: {m}");
    }
    if let Some(m) = hosts.get("rerank_model").and_then(|v| v.as_str()) {
        println!("  rerank_model: {m}");
    }
    if let Some(hints) = hosts.get("hints").and_then(|v| v.as_array()) {
        for h in hints {
            if let Some(s) = h.as_str() {
                println!("  hint: {s}");
            }
        }
    }
}
