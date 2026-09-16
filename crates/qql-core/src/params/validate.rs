//! Validation for unbound parameters in AST statements.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    CollectionConfig, PointId, PointSelector, PointVectors, Prefetch, PrefetchSource, QueryExpr,
    QueryInput, QueryStmt, ShardKey, Stmt, VectorValue,
};
use crate::error::QqlError;

pub use super::validate_collect::{collect_statement_params, stmt_has_point_params};

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
        QueryInput::Text {
            text_param,
            options,
            ..
        } => {
            if let Some(param) = text_param {
                return Err(unbound_param_str_err(param, None));
            }
            validate_no_unbound_options(options)
        }
        QueryInput::Image { options, .. } => validate_no_unbound_options(options),
        QueryInput::Object {
            object, options, ..
        } => {
            validate_no_unbound_value(object)?;
            validate_no_unbound_options(options)
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
        FilterExpr::In { values, .. }
        | FilterExpr::MatchAny { values, .. }
        | FilterExpr::MatchExcept { values, .. } => {
            for v in values {
                validate_no_unbound_value(v)?;
            }
            Ok(())
        }
        FilterExpr::And { operands }
        | FilterExpr::Or { operands }
        | FilterExpr::MinShould { operands, .. } => {
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
    if let Some(spec) = &prefetch.lookup {
        validate_no_unbound_shard_key(&spec.shard_key)?;
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
        QueryExpr::OrderBy { start_from, .. } => {
            if let Some(value) = start_from {
                validate_no_unbound_value(value)?;
            }
            Ok(())
        }
        QueryExpr::SampleRandom => Ok(()),
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
        VectorValue::Document { options, .. } | VectorValue::Image { options, .. } => {
            validate_no_unbound_options(options)
        }
        VectorValue::Object {
            object, options, ..
        } => {
            validate_no_unbound_value(object)?;
            validate_no_unbound_options(options)
        }
        _ => Ok(()),
    }
}

/// Reject unbound placeholders inside an inference `OPTIONS` dict.
fn validate_no_unbound_options(options: &[(String, Value)]) -> Result<(), QqlError> {
    for (_, value) in options {
        validate_no_unbound_value(value)?;
    }
    Ok(())
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
            if let Some(order) = &scroll.order_by
                && let Some(value) = &order.start_from
            {
                validate_no_unbound_value(value)?;
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
            if let Some(filter) = &upsert.update_filter {
                validate_no_unbound_filter(filter)?;
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
            for point in &uv.points {
                validate_no_unbound_point_id(&point.id)?;
                validate_no_unbound_point_vectors(&point.vectors)?;
            }
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
        Stmt::CreateCollection(cc) => {
            validate_collection_shard_keys(
                cc.config
                    .as_ref()
                    .and_then(|config| config.params.as_ref())
                    .and_then(|params| params.shard_keys.as_ref()),
            )?;
            if let Some(config) = &cc.config {
                validate_no_unbound_ddl_options(config)?;
            }
            Ok(())
        }
        Stmt::AlterCollection(ac) => {
            validate_collection_shard_keys(
                ac.config
                    .as_ref()
                    .and_then(|config| config.params.as_ref())
                    .and_then(|params| params.shard_keys.as_ref()),
            )?;
            if let Some(config) = &ac.config {
                validate_no_unbound_ddl_options(config)?;
            }
            Ok(())
        }
        Stmt::Batch(batch) => {
            for member in &batch.statements {
                validate_no_unbound_params(member)?;
            }
            Ok(())
        }
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

/// Reject unbound placeholders in raw `WITH WAL` / `WITH STRICT_MODE` /
/// `WITH METADATA` option pairs (they lower through `value_to_json`, which
/// cannot represent placeholders).
fn validate_no_unbound_ddl_options(config: &CollectionConfig) -> Result<(), QqlError> {
    for options in [&config.wal, &config.strict_mode, &config.metadata]
        .into_iter()
        .flatten()
    {
        for (_, value) in options {
            validate_no_unbound_value(value)?;
        }
    }
    Ok(())
}

fn check_vector_value_template(
    vec: &VectorValue,
    has_vec_params: &mut bool,
) -> Result<(), QqlError> {
    match vec {
        VectorValue::Param(..) | VectorValue::PositionalParam(..) => {
            *has_vec_params = true;
            Ok(())
        }
        VectorValue::Document { options, .. } | VectorValue::Image { options, .. } => {
            validate_no_unbound_options(options)
        }
        VectorValue::Object {
            object, options, ..
        } => {
            validate_no_unbound_value(object)?;
            validate_no_unbound_options(options)
        }
        _ => Ok(()),
    }
}

fn check_point_vectors_template(
    pv: &PointVectors,
    has_vec_params: &mut bool,
) -> Result<(), QqlError> {
    match pv {
        PointVectors::Param(..) | PointVectors::PositionalParam(..) => {
            *has_vec_params = true;
            Ok(())
        }
        PointVectors::Unnamed(v) => check_vector_value_template(v, has_vec_params),
        PointVectors::Named(list) => {
            for (_, v) in list {
                check_vector_value_template(v, has_vec_params)?;
            }
            Ok(())
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
            check_vector_value_template(vec, has_vec_params)?;
            Ok(())
        }
        QueryInput::Point(point) => validate_no_unbound_point_id(point),
        QueryInput::Text {
            text_param,
            options,
            ..
        } => {
            if let Some(param) = text_param {
                Err(unbound_param_str_err(param, None))
            } else {
                validate_no_unbound_options(options)
            }
        }
        QueryInput::Image { options, .. } => validate_no_unbound_options(options),
        QueryInput::Object {
            object, options, ..
        } => {
            validate_no_unbound_value(object)?;
            validate_no_unbound_options(options)
        }
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
        QueryExpr::OrderBy { start_from, .. } => {
            if let Some(value) = start_from {
                validate_no_unbound_value(value)?;
            }
            Ok(())
        }
        QueryExpr::SampleRandom | QueryExpr::Fusion { .. } => Ok(()),
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
                            check_point_vectors_template(vectors, &mut has_vec_params)?;
                        }
                        for (_k, v) in &inline.payload {
                            validate_no_unbound_value(v)?;
                        }
                    }
                }
            }
            if let Some(filter) = &upsert.update_filter {
                validate_no_unbound_filter(filter)?;
            }
            Ok(has_vec_params)
        }
        Stmt::UpdateVector(uv) => {
            for point in &uv.points {
                validate_no_unbound_point_id(&point.id)?;
                check_point_vectors_template(&point.vectors, &mut has_vec_params)?;
            }
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
            if let Some(order) = &scroll.order_by
                && let Some(value) = &order.start_from
            {
                validate_no_unbound_value(value)?;
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
        Stmt::Batch(batch) => {
            for member in &batch.statements {
                has_vec_params |= validate_no_unbound_scalar_params(member)?;
            }
            Ok(has_vec_params)
        }
        _ => {
            validate_no_unbound_params(stmt)?;
            Ok(false)
        }
    }
}
