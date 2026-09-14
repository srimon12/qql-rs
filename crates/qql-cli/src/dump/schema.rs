//! Collection schema generation (`CREATE COLLECTION` statement builder).

use qql::backend::{CollectionInfo, VectorSpec};
use qql_plan::types::MemoryPlacement;
use qql_plan::{HnswConfig, MaxOptimizationThreads, OptimizersConfig};
use serde_json::Value;

use super::escape::{escape_string, format_ident};
use super::indexes::format_index_option;
use super::quant::{format_quantization_config, format_quantization_spec};

pub const HNSW_KEYS: &[&str] = &[
    "m",
    "ef_construct",
    "full_scan_threshold",
    "max_indexing_threads",
    "on_disk",
    "payload_m",
    "inline_storage",
    "memory",
];

pub const SPARSE_INDEX_KEYS: &[&str] = &["full_scan_threshold", "on_disk", "datatype", "memory"];

/// Build a `CREATE COLLECTION` statement from typed collection info.
pub fn generate_create_statement(collection: &str, info: &CollectionInfo) -> String {
    let coll = format_ident(collection);
    let mut parts: Vec<String> = Vec::new();

    for v in &info.schema.vectors {
        parts.push(format_vector_part(v));
    }

    for sv in &info.schema.sparse_vectors {
        parts.push(format_sparse_vector_part(sv));
    }

    let mut stmt = if parts.is_empty() {
        format!("CREATE COLLECTION {}", coll)
    } else {
        format!("CREATE COLLECTION {} ({})", coll, parts.join(", "))
    };

    let mut with_parts = Vec::new();
    let p = &info.schema.params;
    if let Some(n) = p.shard_number {
        with_parts.push(format!("shard_number = {}", n));
    }
    if let Some(ref m) = p.sharding_method {
        with_parts.push(format!("sharding_method = '{}'", escape_string(m)));
    }
    if let Some(b) = p.on_disk_payload {
        with_parts.push(format!("on_disk_payload = {}", b));
    }
    if let Some(ref mem) = p.payload_memory {
        with_parts.push(format!("payload_memory = '{}'", escape_string(mem)));
    }
    if let Some(r) = p.replication_factor {
        with_parts.push(format!("replication_factor = {}", r));
    }
    if !with_parts.is_empty() {
        stmt.push_str(&format!(" WITH PARAMS ({})", with_parts.join(", ")));
    }

    if let Some(ref hnsw) = info.schema.hnsw
        && let Some(block) = format_hnsw_block(hnsw)
    {
        stmt.push_str(&block);
    }

    if let Some(ref opts) = info.schema.optimizers
        && let Some(block) = format_optimizers_block(opts)
    {
        stmt.push_str(&block);
    }

    if let Some(ref quant) = info.schema.quantization {
        stmt.push_str(&format!(
            " WITH QUANTIZATION ({})",
            format_quantization_config(quant)
        ));
    }

    stmt
}

/// `WITH HNSW (…)` from the typed collection-level config.
fn format_hnsw_block(config: &HnswConfig) -> Option<String> {
    let mut opts = Vec::new();
    push_u64(&mut opts, "m", config.m);
    push_u64(&mut opts, "ef_construct", config.ef_construct);
    push_u64(&mut opts, "full_scan_threshold", config.full_scan_threshold);
    push_u64(
        &mut opts,
        "max_indexing_threads",
        config.max_indexing_threads,
    );
    push_bool(&mut opts, "on_disk", config.on_disk);
    push_u64(&mut opts, "payload_m", config.payload_m);
    push_bool(&mut opts, "inline_storage", config.inline_storage);
    push_memory(&mut opts, "memory", config.memory);
    finish_block("HNSW", opts)
}

/// `WITH OPTIMIZERS (…)` from the typed collection-level config.
fn format_optimizers_block(config: &OptimizersConfig) -> Option<String> {
    let mut opts = Vec::new();
    if let Some(value) = config.deleted_threshold
        && let Some(number) = serde_json::Number::from_f64(value)
    {
        opts.push(format!("deleted_threshold = {number}"));
    }
    push_u64(
        &mut opts,
        "vacuum_min_vector_number",
        config.vacuum_min_vector_number,
    );
    push_u64(
        &mut opts,
        "default_segment_number",
        config.default_segment_number,
    );
    push_u64(&mut opts, "max_segment_size", config.max_segment_size);
    push_u64(&mut opts, "memmap_threshold", config.memmap_threshold);
    push_u64(&mut opts, "indexing_threshold", config.indexing_threshold);
    push_u64(&mut opts, "flush_interval_sec", config.flush_interval_sec);
    if let Some(threads) = config.max_optimization_threads {
        opts.push(match threads {
            MaxOptimizationThreads::Auto => "max_optimization_threads = 'auto'".to_string(),
            MaxOptimizationThreads::Threads(count) => {
                format!("max_optimization_threads = {count}")
            }
        });
    }
    push_bool(&mut opts, "prevent_unoptimized", config.prevent_unoptimized);
    finish_block("OPTIMIZERS", opts)
}

/// Emit `key = N`, skipping Qdrant's `0`-means-auto keys.
fn push_u64(opts: &mut Vec<String>, key: &str, value: Option<u64>) {
    let Some(value) = value else {
        return;
    };
    if POSITIVE_ONLY_KEYS.contains(&key) && value == 0 {
        return;
    }
    opts.push(format!("{key} = {value}"));
}

fn push_bool(opts: &mut Vec<String>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        opts.push(format!("{key} = {value}"));
    }
}

fn push_memory(opts: &mut Vec<String>, key: &str, value: Option<MemoryPlacement>) {
    if let Some(value) = value {
        opts.push(format!("{key} = '{}'", value.as_str()));
    }
}

fn finish_block(keyword: &str, opts: Vec<String>) -> Option<String> {
    if opts.is_empty() {
        None
    } else {
        Some(format!(" WITH {} ({})", keyword, opts.join(", ")))
    }
}

fn is_auto_zero(val: &Value) -> bool {
    match val {
        Value::Number(n) => {
            n.as_u64() == Some(0) || n.as_i64() == Some(0) || n.as_f64() == Some(0.0)
        }
        _ => false,
    }
}

/// QQL `config_positive_u64` keys. Qdrant reports `0` for "auto"/unset; emitting
/// `key = 0` fails parse, so dump/migrate omit those zeros.
const POSITIVE_ONLY_KEYS: &[&str] = &[
    "ef_construct",
    "max_indexing_threads",
    "payload_m",
    "vacuum_min_vector_number",
    "default_segment_number",
    "max_segment_size",
    "flush_interval_sec",
];

pub fn format_config_block(
    keyword: &str,
    map: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Option<String> {
    let mut opts = Vec::new();
    for key in allowed {
        if let Some(val) = map.get(*key) {
            if POSITIVE_ONLY_KEYS.contains(key) && is_auto_zero(val) {
                continue;
            }
            if let Some(opt) = format_index_option(key, val) {
                opts.push(opt);
            }
        }
    }
    if opts.is_empty() {
        None
    } else {
        Some(format!(" WITH {} ({})", keyword, opts.join(", ")))
    }
}

pub fn format_sparse_vector_part(sv: &qql::backend::SparseVectorSpec) -> String {
    let mut part = format!("{} SPARSE", format_ident(&sv.name));
    let mut with_opts = Vec::new();
    if let Some(ref modifier) = sv.modifier {
        // Canonicalize modifier for re-parse.
        let m = match modifier.to_ascii_lowercase().as_str() {
            "idf" => "idf".to_string(),
            "none" => "none".to_string(),
            _ => modifier.to_ascii_lowercase(),
        };
        with_opts.push(format!("modifier = '{}'", m));
    }
    if let Some(ref idx) = sv.index {
        for key in SPARSE_INDEX_KEYS {
            if let Some(val) = idx.get(*key) {
                if *key == "datatype" {
                    if let Some(dt) = val.as_str().and_then(normalize_sparse_datatype) {
                        with_opts.push(format!("datatype = '{}'", dt));
                    }
                    continue;
                }
                if let Some(opt) = format_index_option(key, val) {
                    with_opts.push(opt);
                }
            }
        }
    }
    if !with_opts.is_empty() {
        part.push_str(&format!(" WITH SPARSE ({})", with_opts.join(", ")));
    }
    part
}

pub fn normalize_sparse_datatype(s: &str) -> Option<&'static str> {
    match s.to_ascii_lowercase().as_str() {
        "float32" | "f32" => Some("float32"),
        "uint8" | "u8" => Some("uint8"),
        "float16" | "f16" => Some("float16"),
        "default" => Some("default"),
        _ => None,
    }
}

pub fn format_vector_part(v: &VectorSpec) -> String {
    let mut part = match &v.name {
        None => format!("dense VECTOR({}, {})", v.size, distance_token(&v.distance)),
        Some(name) => format!(
            "{} VECTOR({}, {})",
            format_ident(name),
            v.size,
            distance_token(&v.distance)
        ),
    };

    if let Some(ref hnsw) = v.hnsw
        && let Some(block) = format_config_block("HNSW", hnsw, HNSW_KEYS)
    {
        part.push_str(&block);
    }

    if let Some(ref quant) = v.quantization
        && let Some(quant_str) = format_quantization_spec(quant)
    {
        part.push_str(&format!(" WITH QUANTIZATION ({})", quant_str));
    }

    if let Some(ref mv) = v.multivector {
        let comp = mv
            .get("comparator")
            .and_then(|c| c.as_str())
            .unwrap_or("max_sim");
        part.push_str(&format!(" WITH MULTIVECTOR (comparator = '{}')", comp));
    }

    let mut vector_opts = Vec::new();
    if let Some(on_disk) = v.on_disk {
        vector_opts.push(format!("on_disk = {}", on_disk));
    }
    if let Some(ref mem) = v.memory {
        vector_opts.push(format!("memory = '{}'", escape_string(mem)));
    }
    if let Some(ref dt) = v.datatype {
        vector_opts.push(format!("datatype = '{}'", escape_string(dt)));
    }
    if !vector_opts.is_empty() {
        part.push_str(&format!(" WITH VECTOR ({})", vector_opts.join(", ")));
    }

    part
}

pub fn distance_token(distance: &str) -> &'static str {
    match distance.to_ascii_lowercase().as_str() {
        "dot" | "dotproduct" => "DOT",
        "euclid" | "euclidean" => "EUCLID",
        "manhattan" => "MANHATTAN",
        _ => "COSINE",
    }
}
