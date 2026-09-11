//! Formatting for collection DDL statements (CREATE/ALTER/DROP COLLECTION) and vector configs.

pub(crate) use super::index_quota::{
    render_create_index, render_create_shard_key, render_drop_index, render_drop_shard_key,
    render_quantization_block, render_set_quota, render_show_collection, render_show_collections,
    render_show_quotas, render_show_shard_keys,
};

use crate::ast::{
    AlterCollectionStmt, CollectionConfig, CollectionMode, CollectionParamsConfig,
    CreateCollectionStmt, DropCollectionStmt, HnswRuntimeConfig, MultivectorComparator,
    OptimizersRuntimeConfig, SparseVectorDef, SparseVectorDiff, VectorDef, VectorDiff,
    VectorsConfig, escape_string,
};
use crate::fmt::expr::{render_distance, render_f64, render_name};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

pub(crate) fn render_create_collection(statement: &CreateCollectionStmt) -> String {
    let has_vectors = !statement.vectors.is_empty() || !statement.sparse_vectors.is_empty();
    let mode = render_collection_mode(&statement.mode);
    let mut out = format!("CREATE COLLECTION {}", render_name(&statement.collection));
    if !mode.is_empty() && (!has_vectors || statement.vectors.is_empty()) {
        let _ = write!(out, " {}", mode);
    }
    let mut defs = Vec::new();
    for vector in &statement.vectors {
        defs.push(render_vector_def(vector));
    }
    for sparse in &statement.sparse_vectors {
        defs.push(render_sparse_vector_def(sparse));
    }

    let configs = if let Some(config) = &statement.config {
        render_collection_config_clauses(config)
    } else {
        Vec::new()
    };

    let single_line_len = out.len()
        + if has_vectors {
            defs.iter().map(|d| d.len() + 2).sum::<usize>() + 3
        } else {
            0
        }
        + configs.iter().map(|c| c.len() + 1).sum::<usize>();

    let is_complex = defs.len() >= 2
        || (has_vectors && !configs.is_empty())
        || configs.len() >= 2
        || single_line_len > 80;

    if is_complex {
        if has_vectors {
            out.push_str(" (\n  ");
            out.push_str(&defs.join(",\n  "));
            out.push_str("\n)");
        }
        for config_clause in configs {
            out.push('\n');
            out.push_str(&config_clause);
        }
    } else {
        if has_vectors {
            out.push_str(" (");
            out.push_str(&defs.join(", "));
            out.push(')');
        }
        for config_clause in configs {
            out.push(' ');
            out.push_str(&config_clause);
        }
    }
    out
}

pub(crate) fn render_alter_collection(statement: &AlterCollectionStmt) -> String {
    let mut out = format!("ALTER COLLECTION {}", render_name(&statement.collection));
    if let Some(config) = &statement.config {
        let clauses = render_collection_config_clauses(config);
        if clauses.len() >= 2 {
            for clause in clauses {
                out.push('\n');
                out.push_str(&clause);
            }
        } else {
            for clause in clauses {
                out.push(' ');
                out.push_str(&clause);
            }
        }
    }
    out
}

pub(crate) fn render_drop_collection(statement: &DropCollectionStmt) -> String {
    format!("DROP COLLECTION {}", render_name(&statement.collection))
}

// ── DDL config blocks ────────────────────────────────────────────

pub(crate) fn render_collection_config_clauses(config: &CollectionConfig) -> Vec<String> {
    let mut clauses = Vec::new();
    if let Some(hnsw) = &config.hnsw
        && let Some(body) = render_hnsw_block(hnsw)
    {
        clauses.push(format!("WITH HNSW ({})", body));
    }
    if let Some(vectors) = &config.vectors
        && let Some(body) = render_vectors_options(vectors)
    {
        clauses.push(format!("WITH VECTOR ({})", body));
    }
    if let Some(optimizers) = &config.optimizers
        && let Some(body) = render_optimizers_block(optimizers)
    {
        clauses.push(format!("WITH OPTIMIZERS ({})", body));
    }
    if let Some(params) = &config.params
        && let Some(body) = render_params_block(params)
    {
        clauses.push(format!("WITH PARAMS ({})", body));
    }
    if let Some(quantization) = &config.quantization
        && let Some(body) = render_quantization_block(quantization)
    {
        clauses.push(format!("WITH QUANTIZATION ({})", body));
    }
    if let Some(update) = &config.quantization_update {
        if update.disabled {
            clauses.push("WITH QUANTIZATION (disabled = true)".into());
        } else if config.quantization.is_none()
            && let Some(config) = &update.config
            && let Some(body) = render_quantization_block(config)
        {
            clauses.push(format!("WITH QUANTIZATION ({})", body));
        }
    }
    for diff in &config.vector_diffs {
        clauses.push(render_vector_diff(diff));
    }
    for diff in &config.sparse_vector_diffs {
        clauses.push(render_sparse_vector_diff(diff));
    }
    clauses
}

/// `WITH VECTOR <name> (<nested blocks>)` for one dense `ALTER` diff.
pub(crate) fn render_vector_diff(diff: &VectorDiff) -> String {
    let mut blocks = Vec::new();
    if let Some(hnsw) = &diff.hnsw
        && let Some(body) = render_hnsw_block(hnsw)
    {
        blocks.push(format!("HNSW ({})", body));
    }
    if let Some(update) = &diff.quantization {
        if update.disabled {
            blocks.push("QUANTIZATION (disabled = true)".into());
        } else if let Some(config) = &update.config
            && let Some(body) = render_quantization_block(config)
        {
            blocks.push(format!("QUANTIZATION ({})", body));
        }
    }
    if let Some(vectors) = &diff.vectors
        && let Some(body) = render_vectors_options(vectors)
    {
        blocks.push(format!("VECTOR ({})", body));
    }
    format!(
        "WITH VECTOR {} ({})",
        render_name(&diff.name),
        blocks.join(", ")
    )
}

/// `WITH SPARSE <name> (SPARSE (…))` for one sparse `ALTER` diff.
pub(crate) fn render_sparse_vector_diff(diff: &SparseVectorDiff) -> String {
    let mut options = Vec::new();
    if let Some(modifier) = &diff.modifier {
        options.push(format!(
            "modifier = '{}'",
            escape_string(&modifier.to_ascii_lowercase())
        ));
    }
    if let Some(index) = &diff.index {
        if let Some(value) = index.full_scan_threshold {
            options.push(format!("full_scan_threshold = {}", value));
        }
        if let Some(value) = index.on_disk {
            options.push(format!("on_disk = {}", value));
        }
        if let Some(value) = index.datatype {
            options.push(format!("datatype = '{}'", value.as_str()));
        }
        if let Some(value) = index.memory {
            options.push(format!("memory = '{}'", value.as_str()));
        }
    }
    format!(
        "WITH SPARSE {} (SPARSE ({}))",
        render_name(&diff.name),
        options.join(", ")
    )
}

pub(crate) fn render_collection_mode(mode: &CollectionMode) -> String {
    match mode {
        CollectionMode::Dense { model: Some(model) } => {
            format!("USING DENSE MODEL '{}'", escape_string(model))
        }
        CollectionMode::Dense { model: None } => String::new(),
        CollectionMode::Hybrid {
            dense_vector,
            sparse_vector,
        } => {
            let mut out = String::from("HYBRID");
            if let Some(vector) = dense_vector {
                let _ = write!(out, " DENSE VECTOR {}", render_name(vector));
            }
            if let Some(vector) = sparse_vector {
                let _ = write!(out, " SPARSE VECTOR {}", render_name(vector));
            }
            out
        }
        CollectionMode::Rerank => "HYBRID RERANK".into(),
    }
}

pub(crate) fn render_vector_def(vector: &VectorDef) -> String {
    let mut out = format!(
        "{} VECTOR({}, {})",
        render_name(&vector.name),
        vector.size,
        render_distance(vector.distance)
    );
    if let Some(hnsw) = &vector.hnsw
        && let Some(body) = render_hnsw_block(hnsw)
    {
        let _ = write!(out, " WITH HNSW ({})", body);
    }
    if let Some(quantization) = &vector.quantization
        && let Some(body) = render_quantization_block(quantization)
    {
        let _ = write!(out, " WITH QUANTIZATION ({})", body);
    }
    if let Some(multivector) = &vector.multivector {
        let _ = write!(
            out,
            " WITH MULTIVECTOR (comparator = '{}')",
            match multivector.comparator {
                MultivectorComparator::MaxSim => "max_sim",
            }
        );
    }
    if let Some(vectors) = &vector.vectors
        && let Some(body) = render_vectors_options(vectors)
    {
        let _ = write!(out, " WITH VECTOR ({})", body);
    }
    out
}

pub(crate) fn render_vectors_options(vectors: &VectorsConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = vectors.on_disk {
        options.push(format!("on_disk = {}", value));
    }
    if let Some(value) = vectors.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    if let Some(value) = vectors.datatype {
        options.push(format!("datatype = '{}'", value.as_str()));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

pub(crate) fn render_sparse_vector_def(vector: &SparseVectorDef) -> String {
    let mut out = format!("{} SPARSE", render_name(&vector.name));
    let mut options = Vec::new();
    if let Some(modifier) = &vector.modifier {
        options.push(format!(
            "modifier = '{}'",
            escape_string(&modifier.to_ascii_lowercase())
        ));
    }
    if let Some(index) = &vector.index {
        if let Some(value) = index.full_scan_threshold {
            options.push(format!("full_scan_threshold = {}", value));
        }
        if let Some(value) = index.on_disk {
            options.push(format!("on_disk = {}", value));
        }
        if let Some(value) = index.datatype {
            options.push(format!("datatype = '{}'", value.as_str()));
        }
        if let Some(value) = index.memory {
            options.push(format!("memory = '{}'", value.as_str()));
        }
    }
    if !options.is_empty() {
        let _ = write!(out, " WITH SPARSE ({})", options.join(", "));
    }
    out
}

pub(crate) fn render_hnsw_block(hnsw: &HnswRuntimeConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = hnsw.m {
        options.push(format!("m = {}", value));
    }
    if let Some(value) = hnsw.ef_construct {
        options.push(format!("ef_construct = {}", value));
    }
    if let Some(value) = hnsw.full_scan_threshold {
        options.push(format!("full_scan_threshold = {}", value));
    }
    if let Some(value) = hnsw.max_indexing_threads {
        options.push(format!("max_indexing_threads = {}", value));
    }
    if let Some(value) = hnsw.on_disk {
        options.push(format!("on_disk = {}", value));
    }
    if let Some(value) = hnsw.payload_m {
        options.push(format!("payload_m = {}", value));
    }
    if let Some(value) = hnsw.inline_storage {
        options.push(format!("inline_storage = {}", value));
    }
    if let Some(value) = hnsw.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

pub(crate) fn render_optimizers_block(optimizers: &OptimizersRuntimeConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = optimizers.deleted_threshold {
        options.push(format!("deleted_threshold = {}", render_f64(value)));
    }
    if let Some(value) = optimizers.vacuum_min_vector_number {
        options.push(format!("vacuum_min_vector_number = {}", value));
    }
    if let Some(value) = optimizers.default_segment_number {
        options.push(format!("default_segment_number = {}", value));
    }
    if let Some(value) = optimizers.max_segment_size {
        options.push(format!("max_segment_size = {}", value));
    }
    if let Some(value) = optimizers.memmap_threshold {
        options.push(format!("memmap_threshold = {}", value));
    }
    if let Some(value) = optimizers.indexing_threshold {
        options.push(format!("indexing_threshold = {}", value));
    }
    if let Some(value) = optimizers.flush_interval_sec {
        options.push(format!("flush_interval_sec = {}", value));
    }
    if let Some(threads) = &optimizers.max_optimization_threads {
        if threads.auto_ {
            options.push("max_optimization_threads = 'auto'".into());
        } else {
            options.push(format!("max_optimization_threads = {}", threads.value));
        }
    }
    if let Some(value) = optimizers.prevent_unoptimized {
        options.push(format!("prevent_unoptimized = {}", value));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}

pub(crate) fn render_params_block(params: &CollectionParamsConfig) -> Option<String> {
    let mut options = Vec::new();
    if let Some(value) = params.replication_factor {
        options.push(format!("replication_factor = {}", value));
    }
    if let Some(value) = params.write_consistency_factor {
        options.push(format!("write_consistency_factor = {}", value));
    }
    if let Some(value) = params.read_fan_out_factor {
        options.push(format!("read_fan_out_factor = {}", value));
    }
    if let Some(value) = params.read_fan_out_delay_ms {
        options.push(format!("read_fan_out_delay_ms = {}", value));
    }
    if let Some(value) = params.on_disk_payload {
        options.push(format!("on_disk_payload = {}", value));
    }
    if let Some(value) = params.payload_memory {
        options.push(format!("payload_memory = '{}'", value.as_str()));
    }
    if let Some(value) = params.shard_number {
        options.push(format!("shard_number = {}", value));
    }
    if let Some(value) = &params.sharding_method {
        options.push(format!(
            "sharding_method = '{}'",
            escape_string(&value.to_ascii_lowercase())
        ));
    }
    if let Some(values) = &params.shard_keys {
        options.push(format!(
            "shard_keys = [{}]",
            values
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if options.is_empty() {
        None
    } else {
        Some(options.join(", "))
    }
}
