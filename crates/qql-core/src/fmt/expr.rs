//! Formatting for values, literals, selectors, filters, and formula expressions.

use crate::ast::{
    ComparisonOp, FilterExpr, FormulaExpr, GeoPoint, PayloadSelector, PointId, PointIdPredicate,
    ReadConsistency, Value, VectorDistance, VectorSelector, VectorValue, escape_string,
    is_simple_ident,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

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

// ── Filters ──────────────────────────────────────────────────────

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
        _ => render_filter(filter),
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

// ── Formulas ─────────────────────────────────────────────────────

pub(crate) fn render_formula(formula: &FormulaExpr) -> String {
    render_formula_min(formula, 0)
}

fn formula_precedence(formula: &FormulaExpr) -> u8 {
    match formula {
        FormulaExpr::Sum { .. } | FormulaExpr::Sub { .. } => 1,
        FormulaExpr::Mul { .. } | FormulaExpr::Div { .. } => 2,
        FormulaExpr::Neg { .. } => 3,
        _ => 4,
    }
}

fn render_formula_min(formula: &FormulaExpr, min_precedence: u8) -> String {
    let precedence = formula_precedence(formula);
    let rendered = match formula {
        FormulaExpr::Constant { value } => render_f64(*value),
        FormulaExpr::Variable { name } => {
            if name.starts_with('?') {
                "?".to_string()
            } else {
                name.clone()
            }
        }
        FormulaExpr::Sum { left, right } => format!(
            "{} + {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Sub { left, right } => format!(
            "{} - {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Mul { left, right } => format!(
            "{} * {}",
            render_formula_min(left, 2),
            render_formula_min(right, 3)
        ),
        FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let mut out = format!(
                "{} / {}",
                render_formula_min(left, 2),
                render_formula_min(right, 3)
            );
            if let Some(default) = by_zero_default {
                let _ = write!(out, " [DEFAULT = {}]", render_f64(*default));
            }
            out
        }
        FormulaExpr::Neg { operand } => format!("-{}", render_formula_min(operand, 3)),
        FormulaExpr::Abs { x } => format!("ABS({})", render_formula_min(x, 0)),
        FormulaExpr::Sqrt { x } => format!("SQRT({})", render_formula_min(x, 0)),
        FormulaExpr::Log { x } => format!("LOG({})", render_formula_min(x, 0)),
        FormulaExpr::Ln { x } => format!("LN({})", render_formula_min(x, 0)),
        FormulaExpr::Exp { x } => format!("EXP({})", render_formula_min(x, 0)),
        FormulaExpr::Acosh { x } => format!("ACOSH({})", render_formula_min(x, 0)),
        FormulaExpr::Max { args } => render_formula_call("MAX", args),
        FormulaExpr::Min { args } => render_formula_call("MIN", args),
        FormulaExpr::Pow { base, exponent } => format!(
            "POW({}, {})",
            render_formula_min(base, 0),
            render_formula_min(exponent, 0)
        ),
        FormulaExpr::GeoDistance { lat, lon, field } => format!(
            "GEO_DISTANCE({}, {}, {})",
            render_f64(*lat),
            render_f64(*lon),
            render_name(field)
        ),
        FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let mut out = format!("{}(", kind.to_ascii_uppercase());
            out.push_str(&render_formula_min(x, 0));
            if let Some(target) = target {
                let _ = write!(out, ", TARGET = {}", render_formula_min(target, 0));
            }
            if let Some(scale) = scale {
                let _ = write!(out, ", SCALE = {}", render_f64(*scale));
            }
            if let Some(midpoint) = midpoint {
                let _ = write!(out, ", MIDPOINT = {}", render_f64(*midpoint));
            }
            out.push(')');
            out
        }
        FormulaExpr::Case { cond, then_, else_ } => format!(
            "CASE WHEN {} THEN {} ELSE {} END",
            render_filter(cond),
            render_formula_min(then_, 0),
            render_formula_min(else_, 0)
        ),
        FormulaExpr::MatchCondition { field, values } => {
            if values.len() == 1 {
                format!(
                    "MATCH({}, {})",
                    render_name(field),
                    render_value(&values[0])
                )
            } else {
                format!(
                    "MATCH({}, [{}])",
                    render_name(field),
                    values
                        .iter()
                        .map(render_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        FormulaExpr::Datetime { value } => format!("datetime('{}')", escape_string(value)),
        FormulaExpr::DatetimeKey { key } => format!("datetime_key('{}')", escape_string(key)),
    };
    if precedence < min_precedence {
        format!("({})", rendered)
    } else {
        rendered
    }
}

fn render_formula_call(name: &str, args: &[FormulaExpr]) -> String {
    let rendered: Vec<String> = args.iter().map(|a| render_formula_min(a, 0)).collect();
    format!("{}({})", name, rendered.join(", "))
}

// ── Points, values, vectors ──────────────────────────────────────

pub(crate) fn render_point_id(point: &PointId) -> String {
    match point {
        PointId::Number(value) => value.to_string(),
        PointId::String(value) => format!("'{}'", escape_string(value)),
        PointId::Param(name) => format!(":{}", name),
        PointId::PositionalParam(_) => "?".to_string(),
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
        Value::Param(name) => format!(":{}", name),
        Value::PositionalParam(_) => "?".to_string(),
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
    // Keep integral floats as floats (`1.0`), not integers, so the literal
    // re-parses to the same `Value::Float` / `FormulaExpr::Constant`.
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
