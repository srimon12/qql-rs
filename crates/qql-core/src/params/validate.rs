//! Validation for unbound parameters in AST statements.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    PointId, PointSelector, Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt, Stmt,
};
use crate::error::QqlError;

fn unbound_named_err(name: &str) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for named parameter ':{}'", name),
        None,
    )
}

fn unbound_positional_err(idx: usize) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for positional parameter '?{}'", idx),
        None,
    )
}

fn unbound_param_str_err(param: &str) -> QqlError {
    if let Some(name) = param.strip_prefix(':') {
        unbound_named_err(name)
    } else if let Some(idx_str) = param.strip_prefix('?') {
        if let Ok(idx) = idx_str.parse::<usize>() {
            unbound_positional_err(idx)
        } else {
            QqlError::validation(
                "QQL-BIND-INVALID-PARAMS",
                alloc::format!("invalid positional parameter index '?{}'", idx_str),
                None,
            )
        }
    } else {
        unbound_named_err(param)
    }
}

fn validate_no_unbound_value(val: &Value) -> Result<(), QqlError> {
    match val {
        Value::Param(name) => Err(unbound_named_err(name)),
        Value::PositionalParam(idx) => Err(unbound_positional_err(*idx)),
        Value::List(items) => {
            for item in items {
                validate_no_unbound_value(item)?;
            }
            Ok(())
        }
        Value::Dict(entries) => {
            for (_k, v) in entries {
                validate_no_unbound_value(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_no_unbound_point_id(id: &PointId) -> Result<(), QqlError> {
    match id {
        PointId::Param(name) => Err(unbound_named_err(name)),
        PointId::PositionalParam(idx) => Err(unbound_positional_err(*idx)),
        _ => Ok(()),
    }
}

fn validate_no_unbound_query_input(input: &QueryInput) -> Result<(), QqlError> {
    match input {
        QueryInput::Param(name) => Err(unbound_named_err(name)),
        QueryInput::PositionalParam(idx) => Err(unbound_positional_err(*idx)),
        QueryInput::Point(point) => validate_no_unbound_point_id(point),
        QueryInput::Text { text_param, .. } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn validate_no_unbound_filter(filter: &FilterExpr) -> Result<(), QqlError> {
    match filter {
        FilterExpr::PointId(pred) => match pred {
            PointIdPredicate::Eq(id) => validate_no_unbound_point_id(id),
            PointIdPredicate::In(ids) => {
                for id in ids {
                    validate_no_unbound_point_id(id)?;
                }
                Ok(())
            }
        },
        FilterExpr::Compare { value, .. } => validate_no_unbound_value(value),
        FilterExpr::Between { low, high, .. } => {
            validate_no_unbound_value(low)?;
            validate_no_unbound_value(high)
        }
        FilterExpr::In { values, .. } | FilterExpr::MatchAny { values, .. } => {
            for v in values {
                validate_no_unbound_value(v)?;
            }
            Ok(())
        }
        FilterExpr::And { operands } | FilterExpr::Or { operands } => {
            for op in operands {
                validate_no_unbound_filter(op)?;
            }
            Ok(())
        }
        FilterExpr::Not { operand } => validate_no_unbound_filter(operand),
        FilterExpr::Nested { filter, .. } => validate_no_unbound_filter(filter),
        _ => Ok(()),
    }
}

fn validate_no_unbound_formula(expr: &FormulaExpr) -> Result<(), QqlError> {
    match expr {
        FormulaExpr::Variable { name } => {
            if let Some(param_name) = name.strip_prefix(':') {
                return Err(unbound_named_err(param_name));
            } else if let Some(idx_str) = name.strip_prefix('?') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    return Err(unbound_positional_err(idx));
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-INVALID-PARAMS",
                        alloc::format!("invalid positional parameter index '?{}'", idx_str),
                        None,
                    ));
                }
            }
            Ok(())
        }
        FormulaExpr::Sum { left, right }
        | FormulaExpr::Sub { left, right }
        | FormulaExpr::Mul { left, right }
        | FormulaExpr::Div { left, right, .. }
        | FormulaExpr::Pow {
            base: left,
            exponent: right,
        } => {
            validate_no_unbound_formula(left)?;
            validate_no_unbound_formula(right)
        }
        FormulaExpr::Neg { operand }
        | FormulaExpr::Abs { x: operand }
        | FormulaExpr::Sqrt { x: operand }
        | FormulaExpr::Log { x: operand }
        | FormulaExpr::Ln { x: operand }
        | FormulaExpr::Exp { x: operand }
        | FormulaExpr::Acosh { x: operand } => validate_no_unbound_formula(operand),
        FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
            for arg in args {
                validate_no_unbound_formula(arg)?;
            }
            Ok(())
        }
        FormulaExpr::Decay { x, target, .. } => {
            validate_no_unbound_formula(x)?;
            if let Some(t) = target {
                validate_no_unbound_formula(t)?;
            }
            Ok(())
        }
        FormulaExpr::Case { cond, then_, else_ } => {
            validate_no_unbound_filter(cond)?;
            validate_no_unbound_formula(then_)?;
            validate_no_unbound_formula(else_)
        }
        FormulaExpr::MatchCondition { values, .. } => {
            for v in values {
                validate_no_unbound_value(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_no_unbound_prefetch(prefetch: &Prefetch) -> Result<(), QqlError> {
    match &prefetch.source {
        PrefetchSource::Query(q) => validate_no_unbound_query_stmt(q)?,
        PrefetchSource::Cte(_) => {}
    }
    if let Some(f) = &prefetch.filter {
        validate_no_unbound_filter(f)?;
    }
    Ok(())
}

fn validate_no_unbound_query_expr(expr: &QueryExpr) -> Result<(), QqlError> {
    match expr {
        QueryExpr::Points { ids } => {
            for id in ids {
                validate_no_unbound_point_id(id)?;
            }
            Ok(())
        }
        QueryExpr::Nearest {
            input, prefetch, ..
        } => {
            validate_no_unbound_query_input(input)?;
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            for pos in positive {
                validate_no_unbound_query_input(pos)?;
            }
            for neg in negative {
                validate_no_unbound_query_input(neg)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            for pair in pairs {
                validate_no_unbound_query_input(&pair.positive)?;
                validate_no_unbound_query_input(&pair.negative)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            validate_no_unbound_query_input(target)?;
            for pair in context {
                validate_no_unbound_query_input(&pair.positive)?;
                validate_no_unbound_query_input(&pair.negative)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => Ok(()),
        QueryExpr::Fusion { prefetch, .. } => {
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Formula {
            expression,
            defaults,
            prefetch,
        } => {
            validate_no_unbound_formula(expression)?;
            for (_k, v) in defaults {
                validate_no_unbound_value(v)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            validate_no_unbound_query_input(target)?;
            for item in feedback {
                validate_no_unbound_query_input(&item.example)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Hybrid { text_param, .. } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param))
            } else {
                Ok(())
            }
        }
        QueryExpr::Rerank {
            input, prefetch, ..
        } => {
            validate_no_unbound_query_input(input)?;
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::CrossRerank {
            query_param,
            prefetch,
            ..
        } => {
            if let Some(param) = query_param {
                Err(unbound_param_str_err(param))
            } else {
                for p in prefetch {
                    validate_no_unbound_prefetch(p)?;
                }
                Ok(())
            }
        }
    }
}

fn validate_no_unbound_query_stmt(query: &QueryStmt) -> Result<(), QqlError> {
    for cte in &query.ctes {
        validate_no_unbound_query_stmt(&cte.query)?;
    }
    validate_no_unbound_query_expr(&query.expression)?;
    if let Some(filter) = &query.filter {
        validate_no_unbound_filter(filter)?;
    }
    if let Some(param) = &query.page.limit_param {
        return Err(unbound_param_str_err(param));
    }
    if let Some(param) = &query.page.offset_param {
        return Err(unbound_param_str_err(param));
    }
    Ok(())
}

fn validate_no_unbound_point_selector(sel: &PointSelector) -> Result<(), QqlError> {
    match sel {
        PointSelector::Id(id) => validate_no_unbound_point_id(id),
        PointSelector::Ids(ids) => {
            for id in ids {
                validate_no_unbound_point_id(id)?;
            }
            Ok(())
        }
        PointSelector::Filter(f) => validate_no_unbound_filter(f),
    }
}

/// Verify that a statement contains no unbound parameters, without cloning the AST.
pub fn validate_no_unbound_params(stmt: &Stmt) -> Result<(), QqlError> {
    match stmt {
        Stmt::Query(query) => validate_no_unbound_query_stmt(query),
        Stmt::Scroll(scroll) => {
            if let Some(filter) = &scroll.filter {
                validate_no_unbound_filter(filter)?;
            }
            if let Some(after) = &scroll.after {
                validate_no_unbound_point_id(after)?;
            }
            if let Some(param) = &scroll.limit_param {
                return Err(unbound_param_str_err(param));
            }
            Ok(())
        }
        Stmt::Upsert(upsert) => {
            for point in &upsert.points {
                validate_no_unbound_point_id(&point.id)?;
                for (_k, v) in &point.payload {
                    validate_no_unbound_value(v)?;
                }
            }
            Ok(())
        }
        Stmt::Delete(del) => validate_no_unbound_point_selector(&del.selector),
        Stmt::ClearPayload(cp) => validate_no_unbound_point_selector(&cp.selector),
        Stmt::DeletePayload(dp) => validate_no_unbound_point_selector(&dp.selector),
        Stmt::DeleteVector(dv) => validate_no_unbound_point_selector(&dv.selector),
        Stmt::UpdateVector(uv) => validate_no_unbound_point_id(&uv.point_id),
        Stmt::UpdatePayload(up) => {
            validate_no_unbound_point_selector(&up.selector)?;
            for (_k, v) in &up.payload {
                validate_no_unbound_value(v)?;
            }
            Ok(())
        }
        Stmt::Count(count) => {
            if let Some(filter) = &count.filter {
                validate_no_unbound_filter(filter)?;
            }
            Ok(())
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &facet.filter {
                validate_no_unbound_filter(filter)?;
            }
            if let Some(param) = &facet.limit_param {
                return Err(unbound_param_str_err(param));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
