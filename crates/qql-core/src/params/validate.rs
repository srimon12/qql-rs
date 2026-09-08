//! Validation for unbound parameters in AST statements.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    PointId, PointSelector, PointVectors, Prefetch, PrefetchSource, QueryExpr, QueryInput,
    QueryStmt, ShardKey, Stmt, VectorValue,
};
use crate::error::QqlError;

fn unbound_named_err(name: &str, span: Option<crate::error::Span>) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for named parameter ':{}'", name),
        span,
    )
}

fn unbound_positional_err(idx: usize, span: Option<crate::error::Span>) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for positional parameter '?{}'", idx),
        span,
    )
}

fn unbound_param_str_err(param: &str, span: Option<crate::error::Span>) -> QqlError {
    if let Some(name) = param.strip_prefix(':') {
        unbound_named_err(name, span)
    } else if let Some(idx_str) = param.strip_prefix('?') {
        if let Ok(idx) = idx_str.parse::<usize>() {
            unbound_positional_err(idx, span)
        } else {
            QqlError::validation(
                "QQL-BIND-INVALID-PARAMS",
                alloc::format!("invalid positional parameter index '?{}'", idx_str),
                span,
            )
        }
    } else {
        unbound_named_err(param, span)
    }
}

fn validate_no_unbound_value(val: &Value) -> Result<(), QqlError> {
    match val {
        Value::Param(name, span) => Err(unbound_named_err(name, span.as_deref().copied())),
        Value::PositionalParam(idx, span) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
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
        PointId::Param(name, span) => Err(unbound_named_err(name, span.as_deref().copied())),
        PointId::PositionalParam(idx, span) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
        _ => Ok(()),
    }
}

/// Reject an unbound routing-shard placeholder before planning.
///
/// `SHARD :tenant` must be bound like any other placeholder; an unbound key
/// reaching lowering would ship a broken request, so this fails closed with
/// the binder's own missing-param error.
fn validate_no_unbound_shard_key(key: &Option<ShardKey>) -> Result<(), QqlError> {
    match key {
        Some(ShardKey::Param(name, span)) => Err(unbound_named_err(name, span.as_deref().copied())),
        Some(ShardKey::PositionalParam(idx, span)) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
        _ => Ok(()),
    }
}

fn validate_no_unbound_query_input(input: &QueryInput) -> Result<(), QqlError> {
    match input {
        QueryInput::Param(name, span) => Err(unbound_named_err(name, span.as_deref().copied())),
        QueryInput::PositionalParam(idx, span) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
        QueryInput::Point(point) => validate_no_unbound_point_id(point),
        QueryInput::Text { text_param, .. } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param, None))
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
    super::formula::validate_no_unbound_formula(
        expr,
        &validate_no_unbound_filter,
        &validate_no_unbound_value,
    )
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
                Err(unbound_param_str_err(param, None))
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
                Err(unbound_param_str_err(param, None))
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
    validate_no_unbound_shard_key(&query.shard_key)?;
    if let Some(param) = &query.page.limit_param {
        return Err(unbound_param_str_err(param, query.page.limit_span));
    }
    if let Some(param) = &query.page.offset_param {
        return Err(unbound_param_str_err(param, query.page.offset_span));
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

fn validate_no_unbound_vector_value(vec: &VectorValue) -> Result<(), QqlError> {
    match vec {
        VectorValue::Param(name, span) => Err(unbound_named_err(name, span.as_deref().copied())),
        VectorValue::PositionalParam(idx, span) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
        _ => Ok(()),
    }
}

fn validate_no_unbound_point_vectors(pv: &PointVectors) -> Result<(), QqlError> {
    match pv {
        PointVectors::Param(name, span) => Err(unbound_named_err(name, span.as_deref().copied())),
        PointVectors::PositionalParam(idx, span) => {
            Err(unbound_positional_err(*idx, span.as_deref().copied()))
        }
        PointVectors::Unnamed(v) => validate_no_unbound_vector_value(v),
        PointVectors::Named(list) => {
            for (_, v) in list {
                validate_no_unbound_vector_value(v)?;
            }
            Ok(())
        }
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
                return Err(unbound_param_str_err(param, scroll.limit_span));
            }
            validate_no_unbound_shard_key(&scroll.shard_key)?;
            Ok(())
        }
        Stmt::Upsert(upsert) => {
            for point in &upsert.points {
                match point {
                    crate::ast::PointEntry::Inline(inline) => {
                        validate_no_unbound_point_id(&inline.id)?;
                        if let Some(vectors) = &inline.vectors {
                            validate_no_unbound_point_vectors(vectors)?;
                        }
                        for (_k, v) in &inline.payload {
                            validate_no_unbound_value(v)?;
                        }
                    }
                    crate::ast::PointEntry::Param(name, span) => {
                        return Err(unbound_named_err(name, span.as_deref().copied()));
                    }
                    crate::ast::PointEntry::PositionalParam(idx, span) => {
                        return Err(unbound_positional_err(*idx, span.as_deref().copied()));
                    }
                }
            }
            validate_no_unbound_shard_key(&upsert.shard_key)?;
            Ok(())
        }
        Stmt::Delete(del) => {
            validate_no_unbound_point_selector(&del.selector)?;
            validate_no_unbound_shard_key(&del.shard_key)?;
            Ok(())
        }
        Stmt::ClearPayload(cp) => {
            validate_no_unbound_point_selector(&cp.selector)?;
            validate_no_unbound_shard_key(&cp.shard_key)?;
            Ok(())
        }
        Stmt::DeletePayload(dp) => {
            validate_no_unbound_point_selector(&dp.selector)?;
            validate_no_unbound_shard_key(&dp.shard_key)?;
            Ok(())
        }
        Stmt::DeleteVector(dv) => {
            validate_no_unbound_point_selector(&dv.selector)?;
            validate_no_unbound_shard_key(&dv.shard_key)?;
            Ok(())
        }
        Stmt::UpdateVector(uv) => {
            validate_no_unbound_point_id(&uv.point_id)?;
            validate_no_unbound_vector_value(&uv.vector)?;
            validate_no_unbound_shard_key(&uv.shard_key)?;
            Ok(())
        }
        Stmt::UpdatePayload(up) => {
            validate_no_unbound_point_selector(&up.selector)?;
            for (_k, v) in &up.payload {
                validate_no_unbound_value(v)?;
            }
            validate_no_unbound_shard_key(&up.shard_key)?;
            Ok(())
        }
        Stmt::Count(count) => {
            if let Some(filter) = &count.filter {
                validate_no_unbound_filter(filter)?;
            }
            validate_no_unbound_shard_key(&count.shard_key)?;
            Ok(())
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &facet.filter {
                validate_no_unbound_filter(filter)?;
            }
            if let Some(param) = &facet.limit_param {
                return Err(unbound_param_str_err(param, facet.limit_span));
            }
            validate_no_unbound_shard_key(&facet.shard_key)?;
            Ok(())
        }
        Stmt::CreateShardKey(sk) => validate_no_unbound_shard_key(&Some(sk.shard_key.clone())),
        Stmt::DropShardKey(sk) => validate_no_unbound_shard_key(&Some(sk.shard_key.clone())),
        Stmt::CreateCollection(cc) => validate_collection_shard_keys(
            cc.config
                .as_ref()
                .and_then(|config| config.params.as_ref())
                .and_then(|params| params.shard_keys.as_ref()),
        ),
        Stmt::AlterCollection(ac) => validate_collection_shard_keys(
            ac.config
                .as_ref()
                .and_then(|config| config.params.as_ref())
                .and_then(|params| params.shard_keys.as_ref()),
        ),
        _ => Ok(()),
    }
}

/// Reject unbound placeholders in a `WITH PARAMS (shard_keys = …)` list.
fn validate_collection_shard_keys(keys: Option<&Vec<ShardKey>>) -> Result<(), QqlError> {
    if let Some(keys) = keys {
        for key in keys {
            validate_no_unbound_shard_key(&Some(key.clone()))?;
        }
    }
    Ok(())
}

fn check_vector_value_template(vec: &VectorValue, has_vec_params: &mut bool) {
    match vec {
        VectorValue::Param(..) | VectorValue::PositionalParam(..) => {
            *has_vec_params = true;
        }
        _ => {}
    }
}

fn check_point_vectors_template(pv: &PointVectors, has_vec_params: &mut bool) {
    match pv {
        PointVectors::Param(..) | PointVectors::PositionalParam(..) => {
            *has_vec_params = true;
        }
        PointVectors::Unnamed(v) => check_vector_value_template(v, has_vec_params),
        PointVectors::Named(list) => {
            for (_, v) in list {
                check_vector_value_template(v, has_vec_params);
            }
        }
    }
}

fn check_query_input_template(
    input: &QueryInput,
    has_vec_params: &mut bool,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Param(..) | QueryInput::PositionalParam(..) => {
            *has_vec_params = true;
            Ok(())
        }
        QueryInput::Vector(vec) => {
            check_vector_value_template(vec, has_vec_params);
            Ok(())
        }
        QueryInput::Point(point) => validate_no_unbound_point_id(point),
        QueryInput::Text { text_param, .. } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param, None))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn check_query_expr_template(expr: &QueryExpr, has_vec_params: &mut bool) -> Result<(), QqlError> {
    match expr {
        QueryExpr::Points { ids } => {
            for id in ids {
                validate_no_unbound_point_id(id)?;
            }
            Ok(())
        }
        QueryExpr::Nearest { input, .. } => check_query_input_template(input, has_vec_params),
        QueryExpr::Recommend {
            positive, negative, ..
        } => {
            for item in positive.iter().chain(negative.iter()) {
                check_query_input_template(item, has_vec_params)?;
            }
            Ok(())
        }
        QueryExpr::Context { pairs, .. } => {
            for pair in pairs {
                check_query_input_template(&pair.positive, has_vec_params)?;
                check_query_input_template(&pair.negative, has_vec_params)?;
            }
            Ok(())
        }
        QueryExpr::Discover {
            target, context, ..
        } => {
            check_query_input_template(target, has_vec_params)?;
            for pair in context {
                check_query_input_template(&pair.positive, has_vec_params)?;
                check_query_input_template(&pair.negative, has_vec_params)?;
            }
            Ok(())
        }
        QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom | QueryExpr::Fusion { .. } => Ok(()),
        QueryExpr::Formula {
            expression,
            defaults,
            ..
        } => {
            validate_no_unbound_formula(expression)?;
            for (_, val) in defaults {
                validate_no_unbound_value(val)?;
            }
            Ok(())
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            check_query_input_template(target, has_vec_params)?;
            for item in feedback {
                check_query_input_template(&item.example, has_vec_params)?;
            }
            for p in prefetch {
                validate_no_unbound_prefetch(p)?;
            }
            Ok(())
        }
        QueryExpr::Hybrid { text_param, .. } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param, None))
            } else {
                Ok(())
            }
        }
        QueryExpr::Rerank {
            input, prefetch, ..
        } => {
            check_query_input_template(input, has_vec_params)?;
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
                Err(unbound_param_str_err(param, None))
            } else {
                for p in prefetch {
                    validate_no_unbound_prefetch(p)?;
                }
                Ok(())
            }
        }
    }
}

fn validate_no_unbound_scalar_query_stmt(
    query: &QueryStmt,
    has_vec_params: &mut bool,
) -> Result<(), QqlError> {
    for cte in &query.ctes {
        validate_no_unbound_scalar_query_stmt(&cte.query, has_vec_params)?;
    }
    check_query_expr_template(&query.expression, has_vec_params)?;
    if let Some(filter) = &query.filter {
        validate_no_unbound_filter(filter)?;
    }
    if let Some(param) = &query.page.limit_param {
        return Err(unbound_param_str_err(param, query.page.limit_span));
    }
    if let Some(param) = &query.page.offset_param {
        return Err(unbound_param_str_err(param, query.page.offset_span));
    }
    Ok(())
}

/// Verify that a statement contains no unbound scalar parameters, allowing
/// vector parameter placeholders (`VectorValue::Param`, `PointVectors::Param`,
/// `QueryInput::Param`). Returns `Ok(true)` if vector parameters exist.
pub fn validate_no_unbound_scalar_params(stmt: &Stmt) -> Result<bool, QqlError> {
    let mut has_vec_params = false;
    match stmt {
        Stmt::Query(query) => {
            validate_no_unbound_scalar_query_stmt(query, &mut has_vec_params)?;
            Ok(has_vec_params)
        }
        Stmt::Upsert(upsert) => {
            for point in &upsert.points {
                match point {
                    // Whole-point placeholders are bound later (dict splice);
                    // permitted at template time like vector parameters.
                    crate::ast::PointEntry::Param(..)
                    | crate::ast::PointEntry::PositionalParam(..) => {}
                    crate::ast::PointEntry::Inline(inline) => {
                        validate_no_unbound_point_id(&inline.id)?;
                        if let Some(vectors) = &inline.vectors {
                            check_point_vectors_template(vectors, &mut has_vec_params);
                        }
                        for (_k, v) in &inline.payload {
                            validate_no_unbound_value(v)?;
                        }
                    }
                }
            }
            Ok(has_vec_params)
        }
        Stmt::UpdateVector(uv) => {
            validate_no_unbound_point_id(&uv.point_id)?;
            check_vector_value_template(&uv.vector, &mut has_vec_params);
            Ok(has_vec_params)
        }
        // Shard placeholders (`SHARD :tenant`) are bound later like vector
        // parameters, so template planning skips them while still rejecting
        // unbound scalars. Each arm mirrors its full-validation counterpart
        // minus the shard check.
        Stmt::Scroll(scroll) => {
            if let Some(filter) = &scroll.filter {
                validate_no_unbound_filter(filter)?;
            }
            if let Some(after) = &scroll.after {
                validate_no_unbound_point_id(after)?;
            }
            if let Some(param) = &scroll.limit_param {
                return Err(unbound_param_str_err(param, scroll.limit_span));
            }
            Ok(has_vec_params)
        }
        Stmt::Delete(del) => {
            validate_no_unbound_point_selector(&del.selector)?;
            Ok(has_vec_params)
        }
        Stmt::ClearPayload(cp) => {
            validate_no_unbound_point_selector(&cp.selector)?;
            Ok(has_vec_params)
        }
        Stmt::DeletePayload(dp) => {
            validate_no_unbound_point_selector(&dp.selector)?;
            Ok(has_vec_params)
        }
        Stmt::DeleteVector(dv) => {
            validate_no_unbound_point_selector(&dv.selector)?;
            Ok(has_vec_params)
        }
        Stmt::UpdatePayload(up) => {
            validate_no_unbound_point_selector(&up.selector)?;
            for (_k, v) in &up.payload {
                validate_no_unbound_value(v)?;
            }
            Ok(has_vec_params)
        }
        Stmt::Count(count) => {
            if let Some(filter) = &count.filter {
                validate_no_unbound_filter(filter)?;
            }
            Ok(has_vec_params)
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &facet.filter {
                validate_no_unbound_filter(filter)?;
            }
            if let Some(param) = &facet.limit_param {
                return Err(unbound_param_str_err(param, facet.limit_span));
            }
            Ok(has_vec_params)
        }
        _ => {
            validate_no_unbound_params(stmt)?;
            Ok(false)
        }
    }
}

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
            ..
        } => {
            collect_from_param_str(param, named, max_pos);
        }
        _ => {}
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
        FilterExpr::In { values, .. } | FilterExpr::MatchAny { values, .. } => {
            for v in values {
                collect_from_val(v, named, max_pos);
            }
        }
        FilterExpr::And { operands } | FilterExpr::Or { operands } => {
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
            collect_from_point_id(&uv.point_id, &mut named, &mut max_pos);
            collect_from_vector_val(&uv.vector, &mut named, &mut max_pos);
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
        _ => {}
    }
    (named, max_pos)
}
