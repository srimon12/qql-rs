//! Formatting for mutation and point-level statements (UPSERT, DELETE, SCROLL, etc.).

use crate::ast::{
    ClearPayloadStmt, CountStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, EmbedDirective,
    EmbedKind, EmbeddingSpec, FacetStmt, PointEntry, PointSelector, PointVectors, QueryCollection,
    ScrollStmt, UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt, escape_string,
};
use crate::fmt::expr::{
    render_name, render_placeholder, render_point_id, render_value, render_vector_selector,
    render_vector_value,
};
use crate::fmt::filter::render_filter;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

pub(crate) fn render_scroll(statement: &ScrollStmt) -> String {
    let mut out = format!("SCROLL FROM {}", render_name(&statement.collection));
    if let Some(filter) = &statement.filter {
        let _ = write!(out, " WHERE {}", render_filter(filter));
    }
    if let Some(after) = &statement.after {
        let _ = write!(out, " AFTER {}", render_point_id(after));
    }
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(selector) = &statement.with_vector {
        let _ = write!(out, " WITH VECTOR {}", render_vector_selector(selector));
    }
    if statement.limit_param.is_some() {
        if let Some(param) = &statement.limit_param {
            let _ = write!(out, " LIMIT {}", render_placeholder(param));
        }
    } else {
        let _ = write!(out, " LIMIT {}", statement.limit);
    }
    out
}

pub(crate) fn render_upsert(statement: &UpsertStmt) -> String {
    let mut out = format!("UPSERT INTO {} VALUES", render_name(&statement.collection));
    let multiline_points = statement.points.len() > 1;
    for (i, point) in statement.points.iter().enumerate() {
        if multiline_points {
            if i == 0 {
                out.push_str("\n  ");
            } else {
                out.push_str(",\n  ");
            }
        } else if i > 0 {
            out.push_str(", ");
        } else {
            out.push(' ');
        }
        out.push_str(&render_point_entry(point));
    }
    if let Some(embedding) = &statement.embedding {
        let _ = write!(out, " USING {}", render_embedding_spec(embedding));
    }
    if !statement.embed.is_empty() {
        if statement.embed.len() > 1 {
            out.push_str("\nEMBED ");
            for (i, directive) in statement.embed.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n  ");
                }
                out.push_str(&render_embed_directive(directive));
            }
        } else {
            out.push_str(" EMBED ");
            out.push_str(&render_embed_directive(&statement.embed[0]));
        }
    }
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_delete(statement: &DeleteStmt) -> String {
    let mut out = format!(
        "DELETE FROM {} WHERE {}",
        render_name(&statement.collection),
        render_point_selector(&statement.selector)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_clear_payload(statement: &ClearPayloadStmt) -> String {
    let mut out = format!(
        "CLEAR PAYLOAD FROM {} WHERE {}",
        render_name(&statement.collection),
        render_point_selector(&statement.selector)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_delete_payload(statement: &DeletePayloadStmt) -> String {
    let keys: Vec<String> = statement.keys.iter().map(|k| render_name(k)).collect();
    let mut out = format!(
        "DELETE PAYLOAD {} FROM {} WHERE {}",
        keys.join(", "),
        render_name(&statement.collection),
        render_point_selector(&statement.selector)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_delete_vector(statement: &DeleteVectorStmt) -> String {
    let names: Vec<String> = statement
        .vector_names
        .iter()
        .map(|n| render_name(n))
        .collect();
    let mut out = format!(
        "DELETE VECTOR {} FROM {} WHERE {}",
        names.join(", "),
        render_name(&statement.collection),
        render_point_selector(&statement.selector)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_update_vector(statement: &UpdateVectorStmt) -> String {
    let mut out = format!("UPDATE {} SET VECTOR", render_name(&statement.collection));
    if let Some(name) = &statement.vector_name {
        let _ = write!(out, " {}", render_name(name));
    }
    let _ = write!(
        out,
        " = {} WHERE id = {}",
        render_vector_value(&statement.vector),
        render_point_id(&statement.point_id)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_update_payload(statement: &UpdatePayloadStmt) -> String {
    let payload: Vec<String> = statement
        .payload
        .iter()
        .map(|(key, value)| format!("{}: {}", render_name(key), render_value(value)))
        .collect();
    let mut out = format!(
        "UPDATE {} SET PAYLOAD = {{{}}} WHERE {}",
        render_name(&statement.collection),
        payload.join(", "),
        render_point_selector(&statement.selector)
    );
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(wait) = statement.wait {
        let _ = write!(out, " WAIT {}", wait);
    }
    out
}

pub(crate) fn render_count(statement: &CountStmt) -> String {
    let mut out = match &statement.collection {
        QueryCollection::Explicit(collection) => {
            format!("COUNT FROM {}", render_name(collection))
        }
        QueryCollection::Inherited => "COUNT FROM".into(),
    };
    if let Some(filter) = &statement.filter {
        let _ = write!(out, " WHERE {}", render_filter(filter));
    }
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    if let Some(exact) = statement.exact {
        let _ = write!(out, " WITH (exact = {})", exact);
    }
    out
}

pub(crate) fn render_facet(statement: &FacetStmt) -> String {
    let mut out = match &statement.collection {
        QueryCollection::Explicit(collection) => {
            format!(
                "FACET {} FROM {}",
                render_name(&statement.key),
                render_name(collection)
            )
        }
        QueryCollection::Inherited => format!("FACET {}", render_name(&statement.key)),
    };
    if let Some(filter) = &statement.filter {
        let _ = write!(out, " WHERE {}", render_filter(filter));
    }
    if let Some(limit) = statement.limit {
        let _ = write!(out, " LIMIT {}", limit);
    } else if let Some(param) = &statement.limit_param {
        let _ = write!(out, " LIMIT {}", render_placeholder(param));
    }
    if let Some(exact) = statement.exact {
        let _ = write!(out, " EXACT {}", exact);
    }
    if let Some(key) = &statement.shard_key {
        let _ = write!(out, " SHARD '{}'", escape_string(key));
    }
    out
}

pub(crate) fn render_point_selector(selector: &PointSelector) -> String {
    match selector {
        PointSelector::Id(point) => format!("id = {}", render_point_id(point)),
        PointSelector::Ids(points) => format!(
            "id IN ({})",
            points
                .iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        PointSelector::Filter(filter) => render_filter(filter),
    }
}

pub(crate) fn render_point_entry(entry: &PointEntry) -> String {
    match entry {
        PointEntry::Inline(point) => render_point(point),
        PointEntry::Param(name, _) => format!(":{name}"),
        PointEntry::PositionalParam(..) => "?".to_string(),
    }
}

pub(crate) fn render_point(point: &UpsertPoint) -> String {
    let mut parts = vec![format!("id: {}", render_point_id(&point.id))];
    if let Some(vectors) = &point.vectors {
        parts.push(format!("vector: {}", render_point_vectors(vectors)));
    }
    for (key, value) in &point.payload {
        parts.push(format!("{}: {}", render_name(key), render_value(value)));
    }
    format!("{{{}}}", parts.join(", "))
}

pub(crate) fn render_point_vectors(vectors: &PointVectors) -> String {
    match vectors {
        PointVectors::Unnamed(value) => render_vector_value(value),
        PointVectors::Named(pairs) => {
            let entries: Vec<String> = pairs
                .iter()
                .map(|(name, value)| {
                    format!("{}: {}", render_name(name), render_vector_value(value))
                })
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        PointVectors::Param(name, _) => format!(":{}", name),
        PointVectors::PositionalParam(idx, _) => format!("?{}", idx + 1),
    }
}

pub(crate) fn render_embedding_spec(spec: &EmbeddingSpec) -> String {
    match spec {
        EmbeddingSpec::Dense {
            model,
            vector,
            field,
        } => render_embedding_spec_part("DENSE", model, vector, field, false),
        EmbeddingSpec::Sparse {
            model,
            vector,
            field,
        } => render_embedding_spec_part("SPARSE", model, vector, field, false),
        EmbeddingSpec::MultiVector {
            model,
            vector,
            field,
        } => render_embedding_spec_part("MULTIVECTOR", model, vector, field, false),
        EmbeddingSpec::Image {
            model,
            vector,
            field,
        } => render_embedding_spec_part("IMAGE", model, vector, field, false),
        EmbeddingSpec::Hybrid {
            dense_model,
            dense_vector,
            dense_field,
            sparse_model,
            sparse_vector,
            sparse_field,
        } => {
            let mut parts = vec!["HYBRID".to_string()];
            if dense_model.is_some() || dense_vector.is_some() || dense_field.is_some() {
                parts.push(render_embedding_spec_part(
                    "DENSE",
                    dense_model,
                    dense_vector,
                    dense_field,
                    true,
                ));
            }
            if sparse_model.is_some() || sparse_vector.is_some() || sparse_field.is_some() {
                parts.push(render_embedding_spec_part(
                    "SPARSE",
                    sparse_model,
                    sparse_vector,
                    sparse_field,
                    true,
                ));
            }
            parts.join(" ")
        }
        EmbeddingSpec::Multi(specs) => specs
            .iter()
            .map(render_embedding_spec)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub(crate) fn render_embedding_spec_part(
    kind: &str,
    model: &Option<String>,
    vector: &Option<String>,
    field: &Option<String>,
    hybrid: bool,
) -> String {
    let mut parts = vec![kind.to_string()];
    if let Some(model) = model {
        parts.push(format!("MODEL '{}'", escape_string(model)));
    }
    if let Some(field) = field {
        parts.push(format!("ON FIELD {}", render_name(field)));
    }
    if let Some(vector) = vector {
        parts.push(format!(
            "{} {}",
            if hybrid { "VECTOR" } else { "INTO" },
            render_name(vector)
        ));
    }
    parts.join(" ")
}

pub(crate) fn render_embed_directive(directive: &EmbedDirective) -> String {
    let mut out = format!(
        "{} INTO {} USING ",
        render_name(&directive.source_field),
        render_name(&directive.target_vector)
    );
    match &directive.kind {
        EmbedKind::Dense { model } => {
            out.push_str("DENSE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Sparse { model } => {
            out.push_str("SPARSE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Multi { model } => {
            out.push_str("MULTI");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
        EmbedKind::Image { model } => {
            out.push_str("IMAGE");
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
        }
    }
    out
}
