//! Formatting for INDEX, SHARD KEY, SHOW, and QUOTA statements.

use crate::ast::{
    CreateIndexStmt, CreateShardKeyStmt, DropIndexStmt, DropShardKeyStmt, QuantizationConfig,
    QuantizationType, SetQuotaStmt, escape_string,
};
use crate::fmt::expr::{render_f64, render_name, render_value};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

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
