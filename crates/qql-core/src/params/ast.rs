//! AST-level parameter binding across statement nodes.

use super::filter::{bind_filter, bind_point_selector};
use super::formula::bind_formula;
use super::input::{bind_context_pair, bind_feedback_item, bind_query_input};
pub use super::value::{bind_point_id, bind_shard_key, bind_value, resolve_param_u64};

use crate::ast::Value;
use crate::ast::statement::{
    PageSpec, PointEntry, PointVectors, Prefetch, PrefetchSource, QueryExpr, QueryStmt, Stmt,
    UpsertPoint, VectorValue,
};
use crate::error::{QqlError, Span};
use alloc::format;
use alloc::vec::Vec;

/// Bind parameters into a `PageSpec` in-place.
pub fn bind_page_spec<F>(
    page: &mut PageSpec,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    if let Some(param) = page.limit_param.take() {
        let span = page.limit_span.take();
        page.limit = Some(resolve_param_u64(
            &param, span, lookup, positional, "LIMIT", true,
        )?);
    }
    if let Some(param) = page.offset_param.take() {
        let span = page.offset_span.take();
        page.offset = Some(resolve_param_u64(
            &param, span, lookup, positional, "OFFSET", false,
        )?);
    }
    Ok(())
}

fn bind_prefetch<F>(
    prefetch: &mut Prefetch,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match &mut prefetch.source {
        PrefetchSource::Query(sub) => bind_query_stmt(sub, lookup, positional)?,
        PrefetchSource::Cte(_) => {}
    }
    if let Some(f) = &mut prefetch.filter {
        bind_filter(f, lookup, positional)?;
    }
    Ok(())
}

/// Recursively bind parameters into a `QueryExpr` in-place.
pub fn bind_query_expr<F>(
    expr: &mut QueryExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match expr {
        QueryExpr::Points { ids } => {
            for id in ids {
                bind_point_id(id, lookup, positional)?;
            }
        }
        QueryExpr::Nearest {
            input, prefetch, ..
        } => {
            bind_query_input(input, lookup, positional)?;
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            for pos in positive {
                bind_query_input(pos, lookup, positional)?;
            }
            for neg in negative {
                bind_query_input(neg, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            for pair in pairs {
                bind_context_pair(pair, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            bind_query_input(target, lookup, positional)?;
            for pair in context {
                bind_context_pair(pair, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => {}
        QueryExpr::Fusion { prefetch, .. } => {
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Formula {
            expression,
            defaults,
            prefetch,
        } => {
            bind_formula(expression, lookup, positional, &bind_filter)?;
            for (_k, v) in defaults {
                bind_value(v, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            bind_query_input(target, lookup, positional)?;
            for item in feedback {
                bind_feedback_item(item, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Hybrid {
            text, text_param, ..
        } => {
            if let Some(param) = text_param.take() {
                let val = if let Some(param_name) = param.strip_prefix(':') {
                    super::value::resolve_param(param_name, None, lookup)?
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            format!("invalid positional parameter index '?{idx_str}'"),
                            None,
                        )
                    })?;
                    super::value::resolve_positional(idx, None, positional)?
                } else {
                    super::value::resolve_param(&param, None, lookup)?
                };
                if let Value::Str(s) = val {
                    *text = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        format!("parameter '{param}' for HYBRID query must be a string"),
                        None,
                    ));
                }
            }
        }
        QueryExpr::Rerank {
            input, prefetch, ..
        } => {
            bind_query_input(input, lookup, positional)?;
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::CrossRerank {
            query,
            query_param,
            prefetch,
            ..
        } => {
            if let Some(param) = query_param.take() {
                let val = if let Some(param_name) = param.strip_prefix(':') {
                    super::value::resolve_param(param_name, None, lookup)?
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            format!("invalid positional parameter index '?{idx_str}'"),
                            None,
                        )
                    })?;
                    super::value::resolve_positional(idx, None, positional)?
                } else {
                    super::value::resolve_param(&param, None, lookup)?
                };
                if let Value::Str(s) = val {
                    *query = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        format!("parameter '{param}' for CROSS RERANK query must be a string"),
                        None,
                    ));
                }
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
    }
    Ok(())
}

/// Recursively bind parameters into a `QueryStmt` in-place.
pub fn bind_query_stmt<F>(
    query: &mut QueryStmt,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    for cte in &mut query.ctes {
        bind_query_stmt(&mut cte.query, lookup, positional)?;
    }
    bind_query_expr(&mut query.expression, lookup, positional)?;
    if let Some(filter) = &mut query.filter {
        bind_filter(filter, lookup, positional)?;
    }
    bind_page_spec(&mut query.page, lookup, positional)?;
    bind_shard_key(&mut query.shard_key, lookup, positional)?;
    Ok(())
}

/// Bind parameters into a parsed AST `Stmt` in-place.
pub fn bind_stmt<F>(stmt: &mut Stmt, lookup: F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match stmt {
        Stmt::Query(query) => bind_query_stmt(query, &lookup, positional),
        Stmt::Scroll(scroll) => {
            if let Some(filter) = &mut scroll.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            if let Some(after) = &mut scroll.after {
                bind_point_id(after, &lookup, positional)?;
            }
            if let Some(param) = scroll.limit_param.take() {
                let span = scroll.limit_span.take();
                scroll.limit =
                    resolve_param_u64(&param, span, &lookup, positional, "SCROLL LIMIT", true)?;
            }
            bind_shard_key(&mut scroll.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::Upsert(upsert) => {
            // Whole-point placeholders splice in place: one entry may become
            // several points (`:rows` bound to a list of point dicts), so the
            // vec is rebuilt rather than updated in place.
            let mut bound = Vec::with_capacity(upsert.points.len());
            for entry in core::mem::take(&mut upsert.points) {
                match entry {
                    PointEntry::Inline(mut point) => {
                        bind_point_id(&mut point.id, &lookup, positional)?;
                        if let Some(vectors) = &mut point.vectors {
                            bind_point_vectors(vectors, &lookup, positional)?;
                        }
                        for (_k, v) in &mut point.payload {
                            bind_value(v, &lookup, positional)?;
                        }
                        bound.push(PointEntry::Inline(point));
                    }
                    PointEntry::Param(name, span) => {
                        let val = lookup(&name).ok_or_else(|| {
                            QqlError::validation(
                                "QQL-BIND-UNBOUND-PARAM",
                                format!("unbound named parameter ':{name}'"),
                                span.as_deref().copied(),
                            )
                        })?;
                        bind_point_entry_value(
                            &mut bound,
                            val,
                            span.as_deref().copied(),
                            &lookup,
                            positional,
                        )?;
                    }
                    PointEntry::PositionalParam(idx, span) => {
                        let val = positional.get(idx).cloned().ok_or_else(|| {
                            QqlError::validation(
                                "QQL-BIND-MISSING-POSITIONAL",
                                format!("missing positional parameter ?{}", idx + 1),
                                span.as_deref().copied(),
                            )
                        })?;
                        bind_point_entry_value(
                            &mut bound,
                            val,
                            span.as_deref().copied(),
                            &lookup,
                            positional,
                        )?;
                    }
                }
            }
            upsert.points = bound;
            bind_shard_key(&mut upsert.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::Delete(del) => {
            bind_point_selector(&mut del.selector, &lookup, positional)?;
            bind_shard_key(&mut del.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::ClearPayload(cp) => {
            bind_point_selector(&mut cp.selector, &lookup, positional)?;
            bind_shard_key(&mut cp.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::DeletePayload(dp) => {
            bind_point_selector(&mut dp.selector, &lookup, positional)?;
            bind_shard_key(&mut dp.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::DeleteVector(dv) => {
            bind_point_selector(&mut dv.selector, &lookup, positional)?;
            bind_shard_key(&mut dv.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::UpdateVector(uv) => {
            bind_point_id(&mut uv.point_id, &lookup, positional)?;
            bind_vector_value(&mut uv.vector, &lookup, positional)?;
            bind_shard_key(&mut uv.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::UpdatePayload(up) => {
            bind_point_selector(&mut up.selector, &lookup, positional)?;
            for (_k, v) in &mut up.payload {
                bind_value(v, &lookup, positional)?;
            }
            bind_shard_key(&mut up.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::Count(count) => {
            if let Some(filter) = &mut count.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            bind_shard_key(&mut count.shard_key, &lookup, positional)?;
            Ok(())
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &mut facet.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            if let Some(param) = facet.limit_param.take() {
                let span = facet.limit_span.take();
                facet.limit = Some(resolve_param_u64(
                    &param,
                    span,
                    &lookup,
                    positional,
                    "FACET LIMIT",
                    true,
                )?);
            }
            bind_shard_key(&mut facet.shard_key, &lookup, positional)?;
            Ok(())
        }
        other => Err(QqlError::validation(
            "QQL-BIND-UNSUPPORTED-STATEMENT",
            format!(
                "cannot bind parameters into statement type: {}",
                other.stmt_kind()
            ),
            None,
        )),
    }
}

/// Bind parameters into a `VectorValue` in-place.
pub fn bind_vector_value<F>(
    vec: &mut VectorValue,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match vec {
        VectorValue::Param(name, span) => {
            let val = lookup(name).ok_or_else(|| {
                QqlError::validation(
                    "QQL-BIND-UNBOUND-PARAM",
                    format!("unbound named parameter ':{name}'"),
                    span.as_deref().copied(),
                )
            })?;
            *vec = crate::parser::helpers::vector_from_value(val, span.as_deref().copied())?;
        }
        VectorValue::PositionalParam(idx, span) => {
            let val = positional.get(*idx).cloned().ok_or_else(|| {
                QqlError::validation(
                    "QQL-BIND-MISSING-POSITIONAL",
                    format!("missing positional parameter ?{}", *idx + 1),
                    span.as_deref().copied(),
                )
            })?;
            *vec = crate::parser::helpers::vector_from_value(val, span.as_deref().copied())?;
        }
        _ => {}
    }
    Ok(())
}

fn point_param_shape_error(span: Option<Span>) -> QqlError {
    QqlError::validation(
        "QQL-BIND-TYPE-MISMATCH",
        "point parameter must be an object ({id: …, …}) or a list of point objects",
        span,
    )
}

/// Build one concrete point from a bound point-dict value, mirroring
/// `VALUES {…}` row parsing: `id` is required (case-insensitive), `vector`
/// routes through vector lowering, every other key becomes payload.
fn upsert_point_from_items<F>(
    mut row: Vec<(String, Value)>,
    span: Option<Span>,
    lookup: &F,
    positional: &[Value],
) -> Result<UpsertPoint, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let id_index = row
        .iter()
        .position(|(key, _)| key.eq_ignore_ascii_case("id"))
        .ok_or_else(|| {
            QqlError::validation(
                "QQL-VALIDATION-UPSERT-ID",
                "each UPSERT row requires an id",
                span,
            )
        })?;
    let (_, id) = row.remove(id_index);
    let mut id = crate::parser::helpers::point_id_from_value(id, span.unwrap_or(Span::new(0, 0)))?;
    let mut vectors = if let Some(index) = row
        .iter()
        .position(|(key, _)| key.eq_ignore_ascii_case("vector"))
    {
        let (_, value) = row.remove(index);
        Some(crate::parser::helpers::point_vectors_from_value(
            value, span,
        )?)
    } else {
        None
    };
    let mut payload = row;
    // Nested placeholders inside the dict compose like inline rows.
    bind_point_id(&mut id, lookup, positional)?;
    if let Some(vectors) = &mut vectors {
        bind_point_vectors(vectors, lookup, positional)?;
    }
    for (_k, v) in &mut payload {
        bind_value(v, lookup, positional)?;
    }
    Ok(UpsertPoint {
        id,
        vectors,
        payload,
    })
}

/// Splice one bound whole-point value into the output vec: a dict becomes a
/// single point, a list of dicts becomes several points.
fn bind_point_entry_value<F>(
    bound: &mut Vec<PointEntry>,
    value: Value,
    span: Option<Span>,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match value {
        Value::Dict(row) => {
            bound.push(PointEntry::Inline(upsert_point_from_items(
                row, span, lookup, positional,
            )?));
        }
        Value::List(items) => {
            for item in items {
                match item {
                    Value::Dict(row) => {
                        bound.push(PointEntry::Inline(upsert_point_from_items(
                            row, span, lookup, positional,
                        )?));
                    }
                    _ => return Err(point_param_shape_error(span)),
                }
            }
        }
        _ => return Err(point_param_shape_error(span)),
    }
    Ok(())
}

/// Bind parameters into `PointVectors` in-place.
pub fn bind_point_vectors<F>(
    pv: &mut PointVectors,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match pv {
        PointVectors::Param(name, span) => {
            let val = lookup(name).ok_or_else(|| {
                QqlError::validation(
                    "QQL-BIND-UNBOUND-PARAM",
                    format!("unbound named parameter ':{name}'"),
                    span.as_deref().copied(),
                )
            })?;
            *pv = crate::parser::helpers::point_vectors_from_value(val, span.as_deref().copied())?;
        }
        PointVectors::PositionalParam(idx, span) => {
            let val = positional.get(*idx).cloned().ok_or_else(|| {
                QqlError::validation(
                    "QQL-BIND-MISSING-POSITIONAL",
                    format!("missing positional parameter ?{}", *idx + 1),
                    span.as_deref().copied(),
                )
            })?;
            *pv = crate::parser::helpers::point_vectors_from_value(val, span.as_deref().copied())?;
        }
        PointVectors::Unnamed(v) => {
            bind_vector_value(v, lookup, positional)?;
        }
        PointVectors::Named(list) => {
            for (_, v) in list {
                bind_vector_value(v, lookup, positional)?;
            }
        }
    }
    Ok(())
}
