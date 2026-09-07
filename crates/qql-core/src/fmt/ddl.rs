//! Formatting for DDL and administrative statements (CREATE/ALTER/DROP COLLECTION, INDEX, SHARD, etc.).

use crate::ast::{
    AlterCollectionStmt, CollectionConfig, CollectionMode, CollectionParamsConfig,
    CreateCollectionStmt, CreateIndexStmt, CreateShardKeyStmt, DropCollectionStmt, DropIndexStmt,
    DropShardKeyStmt, HnswRuntimeConfig, MultivectorComparator, OptimizersRuntimeConfig,
    QuantizationConfig, QuantizationType, SetQuotaStmt, SparseVectorDef, VectorDef, VectorsConfig,
    escape_string,
};
use crate::fmt::expr::{render_distance, render_f64, render_name, render_value};
use alloc::format;
use alloc::string::String;
use alloc::vec;
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

pub(crate) fn render_create_index(statement: &CreateIndexStmt) -> String {
    let mut out = format!(
        "CREATE INDEX ON COLLECTION {} FOR {} TYPE {}",
        render_name(&statement.collection),
        render_name(&statement.field),
        statement.field_type
    );
    if !statement.options.is_empty() {
        let options: Vec<String> = statement
            .options
            .iter()
            .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
            .collect();
        let _ = write!(out, " WITH ({})", options.join(", "));
    }
    out
}

pub(crate) fn render_drop_index(statement: &DropIndexStmt) -> String {
    format!(
        "DROP INDEX ON COLLECTION {} FOR {}",
        render_name(&statement.collection),
        render_name(&statement.field)
    )
}

pub(crate) fn render_create_shard_key(statement: &CreateShardKeyStmt) -> String {
    let mut out = format!(
        "CREATE SHARD KEY '{}' ON COLLECTION {}",
        escape_string(&statement.shard_key),
        render_name(&statement.collection)
    );
    let mut options = Vec::new();
    if let Some(value) = statement.shards_number {
        options.push(format!("shards_number = {}", value));
    }
    if let Some(value) = statement.replication_factor {
        options.push(format!("replication_factor = {}", value));
    }
    if !options.is_empty() {
        let _ = write!(out, " WITH ({})", options.join(", "));
    }
    out
}

pub(crate) fn render_drop_shard_key(statement: &DropShardKeyStmt) -> String {
    format!(
        "DROP SHARD KEY '{}' ON COLLECTION {}",
        escape_string(&statement.shard_key),
        render_name(&statement.collection)
    )
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

pub(crate) fn render_show_collections() -> String {
    "SHOW COLLECTIONS".into()
}

pub(crate) fn render_show_collection(collection: &str) -> String {
    format!("SHOW COLLECTION {}", render_name(collection))
}

pub(crate) fn render_show_shard_keys(collection: &str) -> String {
    format!("SHOW SHARD KEYS ON COLLECTION {}", render_name(collection))
}

pub(crate) fn render_show_quotas() -> String {
    "SHOW QUOTAS".into()
}

pub(crate) fn render_set_quota(stmt: &SetQuotaStmt) -> String {
    let config: Vec<String> = stmt
        .config
        .iter()
        .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
        .collect();
    let mut out = format!("SET QUOTA ({})", config.join(", "));
    if let Some(wait) = stmt.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
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
    clauses
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
                .map(|s| format!("'{}'", escape_string(s)))
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

pub(crate) fn render_quantization_block(quantization: &QuantizationConfig) -> Option<String> {
    let mut options = vec![format!(
        "type = '{}'",
        render_quantization_type(quantization.qtype)
    )];
    if quantization.always_ram {
        options.push("always_ram = true".into());
    }
    if let Some(value) = quantization.quantile {
        options.push(format!("quantile = {}", render_f64(value)));
    }
    if let Some(value) = quantization.bits {
        options.push(format!("bits = {}", render_f64(value)));
    }
    if let Some(value) = &quantization.compression {
        options.push(format!("compression = '{}'", escape_string(value)));
    }
    if let Some(value) = &quantization.encoding {
        options.push(format!("encoding = '{}'", escape_string(value)));
    }
    if let Some(value) = &quantization.query_encoding {
        options.push(format!("query_encoding = '{}'", escape_string(value)));
    }
    if let Some(value) = quantization.memory {
        options.push(format!("memory = '{}'", value.as_str()));
    }
    Some(options.join(", "))
}

pub(crate) fn render_quantization_type(kind: QuantizationType) -> &'static str {
    match kind {
        QuantizationType::Scalar => "scalar",
        QuantizationType::Binary => "binary",
        QuantizationType::Product => "product",
        QuantizationType::Turbo => "turbo",
    }
}
