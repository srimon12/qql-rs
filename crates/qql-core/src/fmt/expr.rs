//! Formatting for values, literals, selectors, points, and vectors.

pub use crate::fmt::filter::render_filter;

use crate::ast::{
    PayloadSelector, PointId, ReadConsistency, Value, VectorDistance, VectorSelector, VectorValue,
    escape_string, is_simple_ident,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub(crate) fn render_placeholder(param: &str) -> &str {
    if param.starts_with('?') { "?" } else { param }
}

pub(crate) fn render_read_consistency(consistency: &ReadConsistency) -> String {
    match consistency {
        ReadConsistency::Factor(value) => value.to_string(),
        ReadConsistency::Majority => "majority".into(),
        ReadConsistency::Quorum => "quorum".into(),
        ReadConsistency::All => "all".into(),
    }
}

pub(crate) fn render_payload_selector(selector: &PayloadSelector) -> String {
    match selector {
        PayloadSelector::All => "true".into(),
        PayloadSelector::None => "false".into(),
        PayloadSelector::Include(names) => format!(
            "INCLUDE ({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        PayloadSelector::Exclude(names) => format!(
            "EXCLUDE ({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn render_vector_selector(selector: &VectorSelector) -> String {
    match selector {
        VectorSelector::All => "true".into(),
        VectorSelector::None => "false".into(),
        VectorSelector::Names(names) => format!(
            "({})",
            names
                .iter()
                .map(|n| render_name(n))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

// ── Points, values, vectors ──────────────────────────────────────

pub(crate) fn render_point_id(point: &PointId) -> String {
    match point {
        PointId::Number(value) => value.to_string(),
        PointId::String(value) => format!("'{}'", escape_string(value)),
        PointId::Param(name, _) => format!(":{}", name),
        PointId::PositionalParam(..) => "?".to_string(),
    }
}

pub(crate) fn render_vector_value(value: &VectorValue) -> String {
    match value {
        VectorValue::Dense(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|v| render_f32(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        VectorValue::Sparse { indices, values } => format!(
            "{{indices: [{}], values: [{}]}}",
            indices
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            values
                .iter()
                .map(|v| render_f32(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        VectorValue::MultiDense(rows) => format!(
            "[{}]",
            rows.iter()
                .map(|row| format!(
                    "[{}]",
                    row.iter()
                        .map(|v| render_f32(*v))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn render_value(value: &Value) -> String {
    match value {
        Value::Str(value) => format!("'{}'", escape_string(value)),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => render_f64(*value),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".into(),
        Value::Dict(entries) => {
            let items: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{}: {}", render_name(key), render_value(value)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        Value::List(items) => {
            let values: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", values.join(", "))
        }
        Value::Param(name, _) => format!(":{}", name),
        Value::PositionalParam(..) => "?".to_string(),
    }
}

pub(crate) fn render_f32(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e7 {
        format!("{:.1}", value)
    } else {
        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{}.0", rendered)
        }
    }
}

pub(crate) fn render_f64(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{:.1}", value)
    } else {
        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{}.0", rendered)
        }
    }
}

pub(crate) fn render_distance(distance: VectorDistance) -> &'static str {
    match distance {
        VectorDistance::Cosine => "COSINE",
        VectorDistance::Dot => "DOT",
        VectorDistance::Euclid => "EUCLID",
        VectorDistance::Manhattan => "MANHATTAN",
    }
}

/// Format a collection/field/vector name. Simple idents stay bare; anything
/// else (dotted paths, `$`-prefixed variables, hyphens, keywords) is quoted.
pub(crate) fn render_name(name: &str) -> String {
    if is_simple_ident(name) {
        name.to_string()
    } else {
        format!("'{}'", escape_string(name))
    }
}
