//! AST-level parameter binding.

use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    PageSpec, PointId, PointSelector, Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt,
    Stmt,
};
use crate::ast::{Value, looks_like_iso_datetime};
use crate::error::QqlError;

fn resolve_param<F>(name: &str, lookup: &F) -> Result<Value, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let val = lookup(name).ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            alloc::format!("missing value for named parameter ':{}'", name),
            None,
        )
    })?;
    if matches!(val, Value::Null) {
        // A literal `None`/`null` parameter used to render as the text
        // `null`, which then failed downstream with a misleading
        // "query input requires …" parse error. Fail closed here instead.
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            alloc::format!(
                "parameter ':{name}' is null; QQL cannot bind null — pass a concrete value or remove the placeholder"
            ),
            None,
        ));
    }
    Ok(val)
}

fn resolve_positional(idx: usize, positional: &[Value]) -> Result<Value, QqlError> {
    let val = positional.get(idx).cloned().ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            alloc::format!(
                "positional parameter ? index {} out of range (total provided: {})",
                idx + 1,
                positional.len()
            ),
            None,
        )
    })?;
    if matches!(val, Value::Null) {
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            alloc::format!(
                "positional parameter ?{} is null; QQL cannot bind null — pass a concrete value or remove the placeholder",
                idx + 1
            ),
            None,
        ));
    }
    Ok(val)
}

/// Recursively bind parameters into an AST `Value` in-place.
pub fn bind_value<F>(value: &mut Value, lookup: &F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match value {
        Value::Param(name) => {
            let resolved = resolve_param(name, lookup)?;
            *value = resolved;
        }
        Value::PositionalParam(idx) => {
            let resolved = resolve_positional(*idx, positional)?;
            *value = resolved;
        }
        Value::List(items) => {
            for item in items {
                bind_value(item, lookup, positional)?;
            }
        }
        Value::Dict(entries) => {
            for (_k, v) in entries {
                bind_value(v, lookup, positional)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Bind parameters into a `PointId` in-place.
pub fn bind_point_id<F>(id: &mut PointId, lookup: &F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match id {
        PointId::Param(name) => {
            let val = resolve_param(name, lookup)?;
            *id = value_to_point_id(&val)?;
        }
        PointId::PositionalParam(idx) => {
            let val = resolve_positional(*idx, positional)?;
            *id = value_to_point_id(&val)?;
        }
        _ => {}
    }
    Ok(())
}

fn value_to_point_id(val: &Value) -> Result<PointId, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(PointId::Number(*n as u64)),
        Value::Str(s) => Ok(PointId::String(s.clone())),
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-POINT-ID",
            alloc::format!("cannot convert parameter value '{:?}' to point id", val),
            None,
        )),
    }
}

/// Bind parameters into a `QueryInput` in-place.
pub fn bind_query_input<F>(
    input: &mut QueryInput,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match input {
        QueryInput::Param(name) => {
            let val = resolve_param(name, lookup)?;
            *input = value_to_query_input(val)?;
        }
        QueryInput::PositionalParam(idx) => {
            let val = resolve_positional(*idx, positional)?;
            *input = value_to_query_input(val)?;
        }
        QueryInput::Point(point) => {
            bind_point_id(point, lookup, positional)?;
        }
        QueryInput::Text {
            text, text_param, ..
        } => {
            if let Some(param) = text_param.take() {
                if let Some(param_name) = param.strip_prefix(':') {
                    let val = resolve_param(param_name, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for TEXT query must be a string",
                                param_name
                            ),
                            None,
                        ));
                    }
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            alloc::format!("invalid positional parameter index '?{}'", idx_str),
                            None,
                        )
                    })?;
                    let val = resolve_positional(idx, positional)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "positional parameter ?{} for TEXT query must be a string",
                                idx
                            ),
                            None,
                        ));
                    }
                } else {
                    let val = resolve_param(&param, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for TEXT query must be a string",
                                param
                            ),
                            None,
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_to_query_input(val: Value) -> Result<QueryInput, QqlError> {
    match val {
        Value::Str(s) => Ok(QueryInput::Text {
            text: s,
            model: None,
            text_param: None,
        }),
        Value::Int(n) if n >= 0 => Ok(QueryInput::Point(PointId::Number(n as u64))),
        Value::List(_) | Value::Dict(_) => {
            let vec = crate::parser::helpers::vector_from_value(val, None)?;
            Ok(QueryInput::Vector(vec))
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            alloc::format!("unsupported value type for query input: {:?}", val),
            None,
        )),
    }
}

/// Recursively bind parameters into a `FilterExpr` in-place.
pub fn bind_filter<F>(
    filter: &mut FilterExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match filter {
        FilterExpr::PointId(pred) => match pred {
            PointIdPredicate::Eq(id) => bind_point_id(id, lookup, positional)?,
            PointIdPredicate::In(ids) => {
                for id in ids {
                    bind_point_id(id, lookup, positional)?;
                }
            }
        },
        FilterExpr::Compare { value, .. } => {
            bind_value(value, lookup, positional)?;
        }
        FilterExpr::Between { low, high, .. } => {
            bind_value(low, lookup, positional)?;
            bind_value(high, lookup, positional)?;
        }
        FilterExpr::In { values, .. } | FilterExpr::MatchAny { values, .. } => {
            for v in values {
                bind_value(v, lookup, positional)?;
            }
        }
        FilterExpr::And { operands } | FilterExpr::Or { operands } => {
            for op in operands {
                bind_filter(op, lookup, positional)?;
            }
        }
        FilterExpr::Not { operand } => {
            bind_filter(operand, lookup, positional)?;
        }
        FilterExpr::Nested { filter, .. } => {
            bind_filter(filter, lookup, positional)?;
        }
        _ => {}
    }
    Ok(())
}

/// Recursively bind parameters into a `FormulaExpr` in-place.
pub fn bind_formula<F>(
    formula: &mut FormulaExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match formula {
        FormulaExpr::Variable { name } => {
            if let Some(param_name) = name.strip_prefix(':') {
                let val = resolve_param(param_name, lookup)?;
                *formula = value_to_formula_constant(val)?;
            } else if let Some(idx_str) = name.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, positional)?;
                *formula = value_to_formula_constant(val)?;
            }
        }
        FormulaExpr::Sum { left, right }
        | FormulaExpr::Sub { left, right }
        | FormulaExpr::Mul { left, right }
        | FormulaExpr::Div { left, right, .. }
        | FormulaExpr::Pow {
            base: left,
            exponent: right,
        } => {
            bind_formula(left, lookup, positional)?;
            bind_formula(right, lookup, positional)?;
        }
        FormulaExpr::Neg { operand }
        | FormulaExpr::Abs { x: operand }
        | FormulaExpr::Sqrt { x: operand }
        | FormulaExpr::Log { x: operand }
        | FormulaExpr::Ln { x: operand }
        | FormulaExpr::Exp { x: operand }
        | FormulaExpr::Acosh { x: operand } => {
            bind_formula(operand, lookup, positional)?;
        }
        FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
            for arg in args {
                bind_formula(arg, lookup, positional)?;
            }
        }
        FormulaExpr::Decay { x, target, .. } => {
            bind_formula(x, lookup, positional)?;
            if let Some(t) = target {
                bind_formula(t, lookup, positional)?;
            }
        }
        FormulaExpr::Case { cond, then_, else_ } => {
            bind_filter(cond, lookup, positional)?;
            bind_formula(then_, lookup, positional)?;
            bind_formula(else_, lookup, positional)?;
        }
        FormulaExpr::MatchCondition { values, .. } => {
            for v in values {
                bind_value(v, lookup, positional)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_to_u64(val: &Value, clause: &str) -> Result<u64, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(*n as u64),
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-INTEGER",
            alloc::format!("{} parameter must be a non-negative integer", clause),
            None,
        )),
    }
}

/// Like `value_to_u64`, but rejects `0` — for LIMIT-class clauses whose
/// literal form goes through `parse_positive_u64`, so a bound `LIMIT :lim`
/// enforces the same rule the parser enforces for a literal `LIMIT 0`.
fn value_to_positive_u64(val: &Value, clause: &str) -> Result<u64, QqlError> {
    let n = value_to_u64(val, clause)?;
    if n == 0 {
        return Err(QqlError::validation(
            "QQL-BIND-INVALID-INTEGER",
            alloc::format!("{} parameter must be a positive integer", clause),
            None,
        ));
    }
    Ok(n)
}

fn value_to_formula_constant(val: Value) -> Result<FormulaExpr, QqlError> {
    match val {
        Value::Float(f) => Ok(FormulaExpr::Constant { value: f }),
        Value::Int(i) => Ok(FormulaExpr::Constant { value: i as f64 }),
        Value::Str(s) => {
            if looks_like_iso_datetime(&s) {
                Ok(FormulaExpr::Datetime { value: s })
            } else if let Ok(f) = s.parse::<f64>() {
                Ok(FormulaExpr::Constant { value: f })
            } else {
                Ok(FormulaExpr::Variable { name: s })
            }
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-FORMULA-TYPE",
            alloc::format!("formula parameter cannot be bound to value: {:?}", val),
            None,
        )),
    }
}

fn resolve_param_spec<F>(param: &str, lookup: &F, positional: &[Value]) -> Result<Value, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    if let Some(name) = param.strip_prefix(':') {
        resolve_param(name, lookup)
    } else if let Some(idx_str) = param.strip_prefix('?') {
        let idx = idx_str.parse::<usize>().map_err(|_| {
            QqlError::validation(
                "QQL-BIND-INVALID-PARAMS",
                alloc::format!("invalid positional parameter index '?{}'", idx_str),
                None,
            )
        })?;
        resolve_positional(idx, positional)
    } else {
        resolve_param(param, lookup)
    }
}

fn resolve_param_u64<F>(
    param: &str,
    lookup: &F,
    positional: &[Value],
    clause: &str,
    positive: bool,
) -> Result<u64, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let val = resolve_param_spec(param, lookup, positional)?;
    if positive {
        value_to_positive_u64(&val, clause)
    } else {
        value_to_u64(&val, clause)
    }
}

/// Bind parameters into a `PageSpec` in-place.
pub fn bind_page_spec<F>(
    page: &mut PageSpec,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    if let Some(param) = &page.limit_param {
        page.limit = Some(resolve_param_u64(param, lookup, positional, "LIMIT", true)?);
        page.limit_param = None;
    }
    if let Some(param) = &page.offset_param {
        page.offset = Some(resolve_param_u64(
            param, lookup, positional, "OFFSET", false,
        )?);
        page.offset_param = None;
    }
    Ok(())
}

fn bind_point_selector<F>(
    sel: &mut PointSelector,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match sel {
        PointSelector::Id(id) => bind_point_id(id, lookup, positional),
        PointSelector::Ids(ids) => {
            for id in ids {
                bind_point_id(id, lookup, positional)?;
            }
            Ok(())
        }
        PointSelector::Filter(filter) => bind_filter(filter, lookup, positional),
    }
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
                bind_query_input(&mut pair.positive, lookup, positional)?;
                bind_query_input(&mut pair.negative, lookup, positional)?;
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
                bind_query_input(&mut pair.positive, lookup, positional)?;
                bind_query_input(&mut pair.negative, lookup, positional)?;
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
            bind_formula(expression, lookup, positional)?;
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
                bind_query_input(&mut item.example, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Hybrid {
            text, text_param, ..
        } => {
            if let Some(param) = text_param.take() {
                if let Some(param_name) = param.strip_prefix(':') {
                    let val = resolve_param(param_name, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for HYBRID query must be a string",
                                param_name
                            ),
                            None,
                        ));
                    }
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            alloc::format!("invalid positional parameter index '?{}'", idx_str),
                            None,
                        )
                    })?;
                    let val = resolve_positional(idx, positional)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "positional parameter ?{} for HYBRID query must be a string",
                                idx
                            ),
                            None,
                        ));
                    }
                } else {
                    let val = resolve_param(&param, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for HYBRID query must be a string",
                                param
                            ),
                            None,
                        ));
                    }
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
                if let Some(param_name) = param.strip_prefix(':') {
                    let val = resolve_param(param_name, lookup)?;
                    if let Value::Str(s) = val {
                        *query = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for CROSS RERANK query must be a string",
                                param_name
                            ),
                            None,
                        ));
                    }
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            alloc::format!("invalid positional parameter index '?{}'", idx_str),
                            None,
                        )
                    })?;
                    let val = resolve_positional(idx, positional)?;
                    if let Value::Str(s) = val {
                        *query = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "positional parameter ?{} for CROSS RERANK query must be a string",
                                idx
                            ),
                            None,
                        ));
                    }
                } else {
                    let val = resolve_param(&param, lookup)?;
                    if let Value::Str(s) = val {
                        *query = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            alloc::format!(
                                "parameter ':{}' for CROSS RERANK query must be a string",
                                param
                            ),
                            None,
                        ));
                    }
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
                scroll.limit =
                    resolve_param_u64(&param, &lookup, positional, "SCROLL LIMIT", true)?;
            }
            Ok(())
        }
        Stmt::Upsert(upsert) => {
            for point in &mut upsert.points {
                bind_point_id(&mut point.id, &lookup, positional)?;
                for (_k, v) in &mut point.payload {
                    bind_value(v, &lookup, positional)?;
                }
            }
            Ok(())
        }
        Stmt::Delete(del) => bind_point_selector(&mut del.selector, &lookup, positional),
        Stmt::ClearPayload(cp) => bind_point_selector(&mut cp.selector, &lookup, positional),
        Stmt::DeletePayload(dp) => bind_point_selector(&mut dp.selector, &lookup, positional),
        Stmt::DeleteVector(dv) => bind_point_selector(&mut dv.selector, &lookup, positional),
        Stmt::UpdateVector(uv) => bind_point_id(&mut uv.point_id, &lookup, positional),
        Stmt::UpdatePayload(up) => {
            bind_point_selector(&mut up.selector, &lookup, positional)?;
            for (_k, v) in &mut up.payload {
                bind_value(v, &lookup, positional)?;
            }
            Ok(())
        }
        Stmt::Count(count) => {
            if let Some(filter) = &mut count.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            Ok(())
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &mut facet.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            if let Some(param) = facet.limit_param.take() {
                facet.limit = Some(resolve_param_u64(
                    &param,
                    &lookup,
                    positional,
                    "FACET LIMIT",
                    true,
                )?);
            }
            Ok(())
        }
        other => Err(QqlError::validation(
            "QQL-BIND-UNSUPPORTED-STATEMENT",
            alloc::format!(
                "cannot bind parameters into statement type: {}",
                other.stmt_kind()
            ),
            None,
        )),
    }
}
