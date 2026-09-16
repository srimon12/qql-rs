//! Parameter census for AST statements.
//!
//! Split from `validate.rs` (size hygiene): `collect_statement_params` /
//! `stmt_has_point_params` and the private `collect_from_*` walkers. Unlike
//! the validators, collection never fails — it only records which `:named`
//! and `?N` placeholders a statement contains. Behavior is unchanged.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    PointId, PointSelector, PointVectors, PrefetchSource, QueryExpr, QueryInput, QueryStmt,
    ShardKey, Stmt, VectorValue,
};

fn collect_from_val(
    val: &Value,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match val {
        Value::Param(name, _) => {
            named.insert(name.clone());
        }
        Value::PositionalParam(idx, _) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        Value::List(items) => {
            for item in items {
                collect_from_val(item, named, max_pos);
            }
        }
        Value::Dict(entries) => {
            for (_, v) in entries {
                collect_from_val(v, named, max_pos);
            }
        }
        _ => {}
    }
}

fn collect_from_options(
    options: &[(String, Value)],
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    for (_, value) in options {
        collect_from_val(value, named, max_pos);
    }
}

fn collect_from_param_str(
    param: &str,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    if let Some(name) = param.strip_prefix(':') {
        named.insert(alloc::string::String::from(name));
    } else if let Some(idx_str) = param.strip_prefix('?') {
        if let Ok(idx) = idx_str.parse::<usize>() {
            *max_pos = (*max_pos).max(idx + 1);
        } else {
            *max_pos = (*max_pos).max(1);
        }
    } else {
        named.insert(alloc::string::String::from(param));
    }
}

fn collect_from_shard_key(
    key: &Option<ShardKey>,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match key {
        Some(ShardKey::Param(name, _)) => {
            named.insert(name.clone());
        }
        Some(ShardKey::PositionalParam(idx, _)) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        _ => {}
    }
}

fn collect_from_point_id(
    id: &PointId,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match id {
        PointId::Param(name, _) => {
            named.insert(name.clone());
        }
        PointId::PositionalParam(idx, _) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        _ => {}
    }
}

fn collect_from_vector_val(
    vec: &VectorValue,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match vec {
        VectorValue::Param(name, _) => {
            named.insert(name.clone());
        }
        VectorValue::PositionalParam(idx, _) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        VectorValue::Document { options, .. } | VectorValue::Image { options, .. } => {
            collect_from_options(options, named, max_pos);
        }
        VectorValue::Object {
            object, options, ..
        } => {
            collect_from_val(object, named, max_pos);
            collect_from_options(options, named, max_pos);
        }
        _ => {}
    }
}

fn collect_from_point_vectors(
    pv: &PointVectors,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match pv {
        PointVectors::Param(name, _) => {
            named.insert(name.clone());
        }
        PointVectors::PositionalParam(idx, _) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        PointVectors::Unnamed(v) => collect_from_vector_val(v, named, max_pos),
        PointVectors::Named(entries) => {
            for (_, v) in entries {
                collect_from_vector_val(v, named, max_pos);
            }
        }
    }
}

fn collect_from_query_input(
    input: &QueryInput,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match input {
        QueryInput::Param(name, _) => {
            named.insert(name.clone());
        }
        QueryInput::PositionalParam(idx, _) => {
            *max_pos = (*max_pos).max(*idx + 1);
        }
        QueryInput::Vector(vec) => collect_from_vector_val(vec, named, max_pos),
        QueryInput::Point(point) => collect_from_point_id(point, named, max_pos),
        QueryInput::Text {
            text_param: Some(param),
            options,
            ..
        } => {
            collect_from_param_str(param, named, max_pos);
            collect_from_options(options, named, max_pos);
        }
        QueryInput::Text { options, .. } => {
            collect_from_options(options, named, max_pos);
        }
        QueryInput::Image { options, .. } => {
            collect_from_options(options, named, max_pos);
        }
        QueryInput::Object {
            object, options, ..
        } => {
            collect_from_val(object, named, max_pos);
            collect_from_options(options, named, max_pos);
        }
    }
}

fn collect_from_filter(
    filter: &FilterExpr,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match filter {
        FilterExpr::PointId(pred) => match pred {
            PointIdPredicate::Eq(id) => collect_from_point_id(id, named, max_pos),
            PointIdPredicate::In(ids) => {
                for id in ids {
                    collect_from_point_id(id, named, max_pos);
                }
            }
        },
        FilterExpr::Compare { value, .. } => collect_from_val(value, named, max_pos),
        FilterExpr::Between { low, high, .. } => {
            collect_from_val(low, named, max_pos);
            collect_from_val(high, named, max_pos);
        }
        FilterExpr::In { values, .. }
        | FilterExpr::MatchAny { values, .. }
        | FilterExpr::MatchExcept { values, .. } => {
            for v in values {
                collect_from_val(v, named, max_pos);
            }
        }
        FilterExpr::And { operands }
        | FilterExpr::Or { operands }
        | FilterExpr::MinShould { operands, .. } => {
            for op in operands {
                collect_from_filter(op, named, max_pos);
            }
        }
        FilterExpr::Not { operand } => collect_from_filter(operand, named, max_pos),
        FilterExpr::Nested { filter, .. } => collect_from_filter(filter, named, max_pos),
        _ => {}
    }
}

fn collect_from_formula(
    formula: &FormulaExpr,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    match formula {
        FormulaExpr::Variable { name } => {
            collect_from_param_str(name, named, max_pos);
        }
        FormulaExpr::Sum { left, right }
        | FormulaExpr::Sub { left, right }
        | FormulaExpr::Mul { left, right }
        | FormulaExpr::Div { left, right, .. }
        | FormulaExpr::Pow {
            base: left,
            exponent: right,
        } => {
            collect_from_formula(left, named, max_pos);
            collect_from_formula(right, named, max_pos);
        }
        FormulaExpr::Neg { operand }
        | FormulaExpr::Abs { x: operand }
        | FormulaExpr::Sqrt { x: operand }
        | FormulaExpr::Log { x: operand }
        | FormulaExpr::Ln { x: operand }
        | FormulaExpr::Exp { x: operand }
        | FormulaExpr::Acosh { x: operand } => {
            collect_from_formula(operand, named, max_pos);
        }
        FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
            for arg in args {
                collect_from_formula(arg, named, max_pos);
            }
        }
        FormulaExpr::Decay { x, target, .. } => {
            collect_from_formula(x, named, max_pos);
            if let Some(t) = target {
                collect_from_formula(t, named, max_pos);
            }
        }
        FormulaExpr::Case { cond, then_, else_ } => {
            collect_from_filter(cond, named, max_pos);
            collect_from_formula(then_, named, max_pos);
            collect_from_formula(else_, named, max_pos);
        }
        FormulaExpr::MatchCondition { values, .. } => {
            for v in values {
                collect_from_val(v, named, max_pos);
            }
        }
        _ => {}
    }
}

fn collect_from_query_stmt(
    query: &QueryStmt,
    named: &mut alloc::collections::BTreeSet<alloc::string::String>,
    max_pos: &mut usize,
) {
    for cte in &query.ctes {
        collect_from_query_stmt(&cte.query, named, max_pos);
    }
    match &query.expression {
        QueryExpr::Points { ids } => {
            for id in ids {
                collect_from_point_id(id, named, max_pos);
            }
        }
        QueryExpr::Nearest { input, .. } => collect_from_query_input(input, named, max_pos),
        QueryExpr::Recommend {
            positive, negative, ..
        } => {
            for item in positive.iter().chain(negative.iter()) {
                collect_from_query_input(item, named, max_pos);
            }
        }
        QueryExpr::Context { pairs, .. } => {
            for pair in pairs {
                collect_from_query_input(&pair.positive, named, max_pos);
                collect_from_query_input(&pair.negative, named, max_pos);
            }
        }
        QueryExpr::Discover {
            target, context, ..
        } => {
            collect_from_query_input(target, named, max_pos);
            for pair in context {
                collect_from_query_input(&pair.positive, named, max_pos);
                collect_from_query_input(&pair.negative, named, max_pos);
            }
        }
        QueryExpr::OrderBy {
            start_from: Some(value),
            ..
        } => {
            collect_from_val(value, named, max_pos);
        }
        QueryExpr::OrderBy {
            start_from: None, ..
        } => {}
        QueryExpr::Formula {
            expression,
            defaults,
            ..
        } => {
            collect_from_formula(expression, named, max_pos);
            for (_, val) in defaults {
                collect_from_val(val, named, max_pos);
            }
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            collect_from_query_input(target, named, max_pos);
            for item in feedback {
                collect_from_query_input(&item.example, named, max_pos);
            }
            for p in prefetch {
                if let PrefetchSource::Query(sub) = &p.source {
                    collect_from_query_stmt(sub, named, max_pos);
                }
                if let Some(f) = &p.filter {
                    collect_from_filter(f, named, max_pos);
                }
                if let Some(spec) = &p.lookup {
                    collect_from_shard_key(&spec.shard_key, named, max_pos);
                }
            }
        }
        QueryExpr::Hybrid {
            text_param: Some(param),
            ..
        } => {
            collect_from_param_str(param, named, max_pos);
        }
        QueryExpr::Rerank {
            input, prefetch, ..
        } => {
            collect_from_query_input(input, named, max_pos);
            for p in prefetch {
                if let PrefetchSource::Query(sub) = &p.source {
                    collect_from_query_stmt(sub, named, max_pos);
                }
                if let Some(f) = &p.filter {
                    collect_from_filter(f, named, max_pos);
                }
                if let Some(spec) = &p.lookup {
                    collect_from_shard_key(&spec.shard_key, named, max_pos);
                }
            }
        }
        QueryExpr::CrossRerank {
            query_param,
            prefetch,
            ..
        } => {
            if let Some(param) = query_param {
                collect_from_param_str(param, named, max_pos);
            }
            for p in prefetch {
                if let PrefetchSource::Query(sub) = &p.source {
                    collect_from_query_stmt(sub, named, max_pos);
                }
                if let Some(f) = &p.filter {
                    collect_from_filter(f, named, max_pos);
                }
                if let Some(spec) = &p.lookup {
                    collect_from_shard_key(&spec.shard_key, named, max_pos);
                }
            }
        }
        _ => {}
    }
    if let Some(filter) = &query.filter {
        collect_from_filter(filter, named, max_pos);
    }
    collect_from_shard_key(&query.shard_key, named, max_pos);
    if let Some(param) = &query.page.limit_param {
        collect_from_param_str(param, named, max_pos);
    }
    if let Some(param) = &query.page.offset_param {
        collect_from_param_str(param, named, max_pos);
    }
}

/// Whether an upsert template carries whole-point placeholders (`VALUES :p`
/// / `VALUES ?`) needing dict splice at execution time.
///
/// Point-position placeholders are bound to point dicts (or lists of them),
/// unlike every other placeholder kind — executors branch on this to take
/// the point-splice fast path instead of scalar binding.
pub fn stmt_has_point_params(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Batch(batch) => batch.statements.iter().any(stmt_has_point_params),
        Stmt::Upsert(upsert) => upsert.points.iter().any(|point| {
            matches!(
                point,
                crate::ast::PointEntry::Param(..) | crate::ast::PointEntry::PositionalParam(..)
            )
        }),
        _ => false,
    }
}

/// Collect all named parameter names and the maximum positional parameter index
/// present anywhere in a statement AST.
pub fn collect_statement_params(
    stmt: &Stmt,
) -> (alloc::collections::BTreeSet<alloc::string::String>, usize) {
    let mut named = alloc::collections::BTreeSet::new();
    let mut max_pos = 0;
    match stmt {
        Stmt::Query(query) => collect_from_query_stmt(query, &mut named, &mut max_pos),
        Stmt::Scroll(scroll) => {
            if let Some(filter) = &scroll.filter {
                collect_from_filter(filter, &mut named, &mut max_pos);
            }
            if let Some(after) = &scroll.after {
                collect_from_point_id(after, &mut named, &mut max_pos);
            }
            if let Some(order) = &scroll.order_by
                && let Some(value) = &order.start_from
            {
                collect_from_val(value, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&scroll.shard_key, &mut named, &mut max_pos);
            if let Some(param) = &scroll.limit_param {
                collect_from_param_str(param, &mut named, &mut max_pos);
            }
        }
        Stmt::Upsert(upsert) => {
            for point in &upsert.points {
                match point {
                    crate::ast::PointEntry::Inline(inline) => {
                        collect_from_point_id(&inline.id, &mut named, &mut max_pos);
                        if let Some(vectors) = &inline.vectors {
                            collect_from_point_vectors(vectors, &mut named, &mut max_pos);
                        }
                        for (_k, v) in &inline.payload {
                            collect_from_val(v, &mut named, &mut max_pos);
                        }
                    }
                    crate::ast::PointEntry::Param(name, _) => {
                        named.insert(name.clone());
                    }
                    crate::ast::PointEntry::PositionalParam(idx, _) => {
                        max_pos = max_pos.max(*idx + 1);
                    }
                }
            }
            if let Some(filter) = &upsert.update_filter {
                collect_from_filter(filter, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&upsert.shard_key, &mut named, &mut max_pos);
        }
        Stmt::Delete(del) => {
            match &del.selector {
                PointSelector::Id(id) => collect_from_point_id(id, &mut named, &mut max_pos),
                PointSelector::Ids(ids) => {
                    for id in ids {
                        collect_from_point_id(id, &mut named, &mut max_pos);
                    }
                }
                PointSelector::Filter(f) => collect_from_filter(f, &mut named, &mut max_pos),
            }
            collect_from_shard_key(&del.shard_key, &mut named, &mut max_pos);
        }
        Stmt::ClearPayload(cp) => {
            match &cp.selector {
                PointSelector::Id(id) => collect_from_point_id(id, &mut named, &mut max_pos),
                PointSelector::Ids(ids) => {
                    for id in ids {
                        collect_from_point_id(id, &mut named, &mut max_pos);
                    }
                }
                PointSelector::Filter(f) => collect_from_filter(f, &mut named, &mut max_pos),
            }
            collect_from_shard_key(&cp.shard_key, &mut named, &mut max_pos);
        }
        Stmt::DeletePayload(dp) => {
            match &dp.selector {
                PointSelector::Id(id) => collect_from_point_id(id, &mut named, &mut max_pos),
                PointSelector::Ids(ids) => {
                    for id in ids {
                        collect_from_point_id(id, &mut named, &mut max_pos);
                    }
                }
                PointSelector::Filter(f) => collect_from_filter(f, &mut named, &mut max_pos),
            }
            collect_from_shard_key(&dp.shard_key, &mut named, &mut max_pos);
        }
        Stmt::DeleteVector(dv) => {
            match &dv.selector {
                PointSelector::Id(id) => collect_from_point_id(id, &mut named, &mut max_pos),
                PointSelector::Ids(ids) => {
                    for id in ids {
                        collect_from_point_id(id, &mut named, &mut max_pos);
                    }
                }
                PointSelector::Filter(f) => collect_from_filter(f, &mut named, &mut max_pos),
            }
            collect_from_shard_key(&dv.shard_key, &mut named, &mut max_pos);
        }
        Stmt::UpdateVector(uv) => {
            for point in &uv.points {
                collect_from_point_id(&point.id, &mut named, &mut max_pos);
                collect_from_point_vectors(&point.vectors, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&uv.shard_key, &mut named, &mut max_pos);
        }
        Stmt::UpdatePayload(up) => {
            match &up.selector {
                PointSelector::Id(id) => collect_from_point_id(id, &mut named, &mut max_pos),
                PointSelector::Ids(ids) => {
                    for id in ids {
                        collect_from_point_id(id, &mut named, &mut max_pos);
                    }
                }
                PointSelector::Filter(f) => collect_from_filter(f, &mut named, &mut max_pos),
            }
            for (_k, v) in &up.payload {
                collect_from_val(v, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&up.shard_key, &mut named, &mut max_pos);
        }
        Stmt::Count(count) => {
            if let Some(filter) = &count.filter {
                collect_from_filter(filter, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&count.shard_key, &mut named, &mut max_pos);
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &facet.filter {
                collect_from_filter(filter, &mut named, &mut max_pos);
            }
            collect_from_shard_key(&facet.shard_key, &mut named, &mut max_pos);
            if let Some(param) = &facet.limit_param {
                collect_from_param_str(param, &mut named, &mut max_pos);
            }
        }
        Stmt::Batch(batch) => {
            for member in &batch.statements {
                let (names, pos) = collect_statement_params(member);
                named.extend(names);
                max_pos = max_pos.max(pos);
            }
        }
        _ => {}
    }
    (named, max_pos)
}
