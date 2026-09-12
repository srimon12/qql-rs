//! Formatting for filter expressions.

use crate::ast::{ComparisonOp, FilterExpr, GeoPoint, PointIdPredicate, escape_string};
use crate::fmt::expr::{render_f64, render_name, render_point_id, render_value};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Render a filter expression in canonical QQL form.
pub fn render_filter(filter: &FilterExpr) -> String {
    match filter {
        FilterExpr::And { operands } => operands
            .iter()
            .map(|operand| {
                if matches!(operand, FilterExpr::And { .. } | FilterExpr::Or { .. }) {
                    format!("({})", render_filter(operand))
                } else {
                    render_filter(operand)
                }
            })
            .collect::<Vec<_>>()
            .join(" AND "),
        FilterExpr::Or { operands } => operands
            .iter()
            .map(|operand| {
                if matches!(operand, FilterExpr::Or { .. }) {
                    format!("({})", render_filter(operand))
                } else {
                    render_filter(operand)
                }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
        FilterExpr::Not { operand } => {
            if matches!(
                operand.as_ref(),
                FilterExpr::And { .. } | FilterExpr::Or { .. }
            ) {
                format!("NOT ({})", render_filter(operand))
            } else {
                format!("NOT {}", render_filter(operand))
            }
        }
        predicate => render_filter_predicate(predicate),
    }
}

fn render_filter_predicate(filter: &FilterExpr) -> String {
    match filter {
        FilterExpr::PointId(PointIdPredicate::Eq(point)) => {
            format!("id = {}", render_point_id(point))
        }
        FilterExpr::PointId(PointIdPredicate::In(points)) => format!(
            "id IN ({})",
            points
                .iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::Compare { field, op, value } => format!(
            "{} {} {}",
            render_name(field),
            render_comparison_op(*op),
            render_value(value)
        ),
        FilterExpr::Between { field, low, high } => format!(
            "{} BETWEEN {} AND {}",
            render_name(field),
            render_value(low),
            render_value(high)
        ),
        FilterExpr::In { field, values } => format!(
            "{} IN ({})",
            render_name(field),
            values
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::IsNull { field } => format!("{} IS NULL", render_name(field)),
        FilterExpr::IsEmpty { field } => format!("{} IS EMPTY", render_name(field)),
        FilterExpr::MatchText { field, text } => {
            format!("{} MATCH '{}'", render_name(field), escape_string(text))
        }
        FilterExpr::MatchAny { field, values } => format!(
            "{} MATCH ANY ({})",
            render_name(field),
            values
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FilterExpr::MatchPhrase { field, text } => format!(
            "{} MATCH PHRASE '{}'",
            render_name(field),
            escape_string(text)
        ),
        FilterExpr::MatchPrefix { field, prefix } => format!(
            "{} MATCH PREFIX '{}'",
            render_name(field),
            escape_string(prefix)
        ),
        FilterExpr::Nested { path, filter } => format!(
            "NESTED('{}', {})",
            escape_string(path),
            render_filter(filter)
        ),
        FilterExpr::HasVector { name } => format!("HAS_VECTOR {}", render_name(name)),
        FilterExpr::Slice { total, index } => format!("SLICE ({}, {})", total, index),
        FilterExpr::ValuesCount { field, op, count } => format!(
            "{} VALUES_COUNT {} {}",
            render_name(field),
            render_comparison_op(*op),
            count
        ),
        FilterExpr::GeoBoundingBox {
            field,
            top_left,
            bottom_right,
        } => format!(
            "{} GEO_BBOX {{top_left: {{lat: {}, lon: {}}}, bottom_right: {{lat: {}, lon: {}}}}}",
            render_name(field),
            render_f64(top_left.lat),
            render_f64(top_left.lon),
            render_f64(bottom_right.lat),
            render_f64(bottom_right.lon)
        ),
        FilterExpr::GeoRadius {
            field,
            center,
            radius,
        } => format!(
            "{} GEO_RADIUS {{center: {{lat: {}, lon: {}}}, radius: {}}}",
            render_name(field),
            render_f64(center.lat),
            render_f64(center.lon),
            render_f64(*radius)
        ),
        FilterExpr::GeoPolygon {
            field,
            exterior,
            interiors,
        } => {
            let mut out = format!(
                "{} GEO_POLYGON {{exterior: [{}]",
                render_name(field),
                render_geo_ring(exterior)
            );
            if !interiors.is_empty() {
                let _ = write!(
                    out,
                    ", interiors: [{}]",
                    interiors
                        .iter()
                        .map(|ring| format!("[{}]", render_geo_ring(ring)))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            out.push('}');
            out
        }
        FilterExpr::And { .. } | FilterExpr::Or { .. } | FilterExpr::Not { .. } => {
            render_filter(filter)
        }
    }
}

fn render_geo_ring(points: &[GeoPoint]) -> String {
    points
        .iter()
        .map(|point| {
            format!(
                "{{lat: {}, lon: {}}}",
                render_f64(point.lat),
                render_f64(point.lon)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn render_comparison_op(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Eq => "=",
        ComparisonOp::Gt => ">",
        ComparisonOp::Gte => ">=",
        ComparisonOp::Lt => "<",
        ComparisonOp::Lte => "<=",
    }
}
