//! Single-pass parameter census for AST statements.
//!
//! One traversal produces every parameter answer: the named/positional census
//! (`collect_statement_params`), the whole-point flag (`stmt_has_point_params`),
//! the full unbound check (`validate_no_unbound_params`), and the template
//! check (`validate_no_unbound_scalar_params` with its vector-param flag).
//! The three historical walkers covered one `Stmt` shape and drifted apart
//! (missing prefetch arms, DDL gaps, `QueryInput::Vector` handling). This
//! module visits each node once and folds all answers from that visit.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    CollectionConfig, PointId, PointSelector, PointVectors, Prefetch, PrefetchSource, QueryExpr,
    QueryInput, QueryStmt, ShardKey, Stmt, VectorValue,
};
use crate::error::{QqlError, Span};

fn unbound_named_err(name: &str, span: Option<Span>) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for named parameter ':{}'", name),
        span,
    )
}

fn unbound_positional_err(idx: usize, span: Option<Span>) -> QqlError {
    QqlError::validation(
        "QQL-BIND-MISSING-PARAM",
        alloc::format!("missing value for positional parameter '?{}'", idx),
        span,
    )
}

fn unbound_param_str_err(param: &str, span: Option<Span>) -> QqlError {
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

/// Every parameter answer for one statement, gathered in a single walk.
pub(crate) struct Census {
    pub(crate) named: alloc::collections::BTreeSet<alloc::string::String>,
    pub(crate) max_pos: usize,
    pub(crate) has_vec_params: bool,
    pub(crate) has_point_params: bool,
    pub(crate) full_err: Option<QqlError>,
    pub(crate) scalar_err: Option<QqlError>,
}

impl Census {
    fn new() -> Self {
        Self {
            named: alloc::collections::BTreeSet::new(),
            max_pos: 0,
            has_vec_params: false,
            has_point_params: false,
            full_err: None,
            scalar_err: None,
        }
    }

    fn record_named(&mut self, name: &str) {
        self.named.insert(alloc::string::String::from(name));
    }

    fn record_pos(&mut self, idx: usize) {
        self.max_pos = self.max_pos.max(idx + 1);
    }

    fn note_full(&mut self, err: QqlError) {
        if self.full_err.is_none() {
            self.full_err = Some(err);
        }
    }

    fn note_scalar(&mut self, err: QqlError) {
        if self.scalar_err.is_none() {
            self.scalar_err = Some(err);
        }
    }

    fn note_both(&mut self, full: QqlError, scalar: QqlError) {
        self.note_full(full);
        self.note_scalar(scalar);
    }

    /// Scalar placeholder (`Value`, point ID, `OPTIONS` entry, page limit).
    /// Both validators reject it; the census records it.
    fn scalar_named(&mut self, name: &str, span: Option<Span>) {
        self.record_named(name);
        let full = unbound_named_err(name, span);
        let scalar = unbound_named_err(name, span);
        self.note_both(full, scalar);
    }

    fn scalar_pos(&mut self, idx: usize, span: Option<Span>) {
        self.record_pos(idx);
        let full = unbound_positional_err(idx, span);
        let scalar = unbound_positional_err(idx, span);
        self.note_both(full, scalar);
    }

    /// Vector-tolerant placeholder (`QueryInput` / `VectorValue` /
    /// `PointVectors` whole-slot params). Full validation rejects it; the
    /// template check records the vector flag instead.
    fn vec_named(&mut self, name: &str, span: Option<Span>) {
        self.record_named(name);
        self.has_vec_params = true;
        self.note_full(unbound_named_err(name, span));
    }

    fn vec_pos(&mut self, idx: usize, span: Option<Span>) {
        self.record_pos(idx);
        self.has_vec_params = true;
        self.note_full(unbound_positional_err(idx, span));
    }

    /// Routing-shard placeholder. Full validation rejects it; templates bind
    /// it later like vector params, so the scalar check skips it.
    fn shard_named(&mut self, name: &str, span: Option<Span>) {
        self.record_named(name);
        self.note_full(unbound_named_err(name, span));
    }

    fn shard_pos(&mut self, idx: usize, span: Option<Span>) {
        self.record_pos(idx);
        self.note_full(unbound_positional_err(idx, span));
    }

    /// `":name"` / `"?N"` string placeholder (text params, page limits,
    /// hybrid/cross-rerank query text). Always scalar in both validators.
    fn param_str(&mut self, param: &str, span: Option<Span>) {
        if let Some(name) = param.strip_prefix(':') {
            self.record_named(name);
        } else if let Some(idx_str) = param.strip_prefix('?') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                self.record_pos(idx);
            } else {
                self.max_pos = self.max_pos.max(1);
            }
        } else {
            self.record_named(param);
        }
        let full = unbound_param_str_err(param, span);
        let scalar = unbound_param_str_err(param, span);
        self.note_both(full, scalar);
    }

    /// Formula `Variable` name. Only `:` / `?` prefixed names are
    /// placeholders; bare names (`$score`, `rank`) are variables or
    /// `DEFAULTS` keys and contribute nothing.
    fn formula_var(&mut self, name: &str) {
        if let Some(param_name) = name.strip_prefix(':') {
            self.record_named(param_name);
            let full = unbound_named_err(param_name, None);
            let scalar = unbound_named_err(param_name, None);
            self.note_both(full, scalar);
        } else if let Some(idx_str) = name.strip_prefix('?') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                self.record_pos(idx);
                let full = unbound_positional_err(idx, None);
                let scalar = unbound_positional_err(idx, None);
                self.note_both(full, scalar);
            } else {
                self.max_pos = self.max_pos.max(1);
                let full = unbound_param_str_err(name, None);
                let scalar = unbound_param_str_err(name, None);
                self.note_both(full, scalar);
            }
        }
    }

    fn value(&mut self, val: &Value) {
        match val {
            Value::Param(name, span) => {
                self.scalar_named(name, span.as_deref().copied());
            }
            Value::PositionalParam(idx, span) => {
                self.scalar_pos(*idx, span.as_deref().copied());
            }
            Value::List(items) => {
                for item in items {
                    self.value(item);
                }
            }
            Value::Dict(entries) => {
                for (_, v) in entries {
                    self.value(v);
                }
            }
            _ => {}
        }
    }

    fn options(&mut self, options: &[(alloc::string::String, Value)]) {
        for (_, v) in options {
            self.value(v);
        }
    }

    fn point_id(&mut self, id: &PointId) {
        match id {
            PointId::Param(name, span) => {
                self.scalar_named(name, span.as_deref().copied());
            }
            PointId::PositionalParam(idx, span) => {
                self.scalar_pos(*idx, span.as_deref().copied());
            }
            _ => {}
        }
    }

    fn shard_opt(&mut self, key: &Option<ShardKey>) {
        match key {
            Some(ShardKey::Param(name, span)) => {
                self.shard_named(name, span.as_deref().copied());
            }
            Some(ShardKey::PositionalParam(idx, span)) => {
                self.shard_pos(*idx, span.as_deref().copied());
            }
            _ => {}
        }
    }

    fn shard_list(&mut self, keys: Option<&alloc::vec::Vec<ShardKey>>) {
        if let Some(keys) = keys {
            for key in keys {
                match key {
                    ShardKey::Param(name, span) => {
                        let span = span.as_deref().copied();
                        self.record_named(name);
                        let full = unbound_named_err(name, span);
                        let scalar = unbound_named_err(name, span);
                        self.note_both(full, scalar);
                    }
                    ShardKey::PositionalParam(idx, span) => {
                        let span = span.as_deref().copied();
                        self.record_pos(*idx);
                        let full = unbound_positional_err(*idx, span);
                        let scalar = unbound_positional_err(*idx, span);
                        self.note_both(full, scalar);
                    }
                    _ => {}
                }
            }
        }
    }

    fn ddl_options(&mut self, config: &CollectionConfig) {
        for options in [&config.wal, &config.strict_mode, &config.metadata]
            .into_iter()
            .flatten()
        {
            for (_, v) in options {
                self.value(v);
            }
        }
    }

    fn vector_value(&mut self, vec: &VectorValue) {
        match vec {
            VectorValue::Param(name, span) => {
                self.vec_named(name, span.as_deref().copied());
            }
            VectorValue::PositionalParam(idx, span) => {
                self.vec_pos(*idx, span.as_deref().copied());
            }
            VectorValue::Document { options, .. } | VectorValue::Image { options, .. } => {
                self.options(options);
            }
            VectorValue::Object {
                object, options, ..
            } => {
                self.value(object);
                self.options(options);
            }
            _ => {}
        }
    }

    fn point_vectors(&mut self, pv: &PointVectors) {
        match pv {
            PointVectors::Param(name, span) => {
                self.vec_named(name, span.as_deref().copied());
            }
            PointVectors::PositionalParam(idx, span) => {
                self.vec_pos(*idx, span.as_deref().copied());
            }
            PointVectors::Unnamed(v) => self.vector_value(v),
            PointVectors::Named(list) => {
                for (_, v) in list {
                    self.vector_value(v);
                }
            }
        }
    }

    fn query_input(&mut self, input: &QueryInput) {
        match input {
            QueryInput::Param(name, span) => {
                self.vec_named(name, span.as_deref().copied());
            }
            QueryInput::PositionalParam(idx, span) => {
                self.vec_pos(*idx, span.as_deref().copied());
            }
            QueryInput::Vector(vec) => self.vector_value(vec),
            QueryInput::Point(point) => self.point_id(point),
            QueryInput::Text {
                text_param: Some(param),
                options,
                ..
            } => {
                self.param_str(param, None);
                self.options(options);
            }
            QueryInput::Text { options, .. } => self.options(options),
            QueryInput::Image { options, .. } => self.options(options),
            QueryInput::Object {
                object, options, ..
            } => {
                self.value(object);
                self.options(options);
            }
        }
    }

    fn filter(&mut self, filter: &FilterExpr) {
        match filter {
            FilterExpr::PointId(pred) => match pred {
                PointIdPredicate::Eq(id) => self.point_id(id),
                PointIdPredicate::In(ids) => {
                    for id in ids {
                        self.point_id(id);
                    }
                }
            },
            FilterExpr::Compare { value, .. } => self.value(value),
            FilterExpr::Between { low, high, .. } => {
                self.value(low);
                self.value(high);
            }
            FilterExpr::In { values, .. }
            | FilterExpr::MatchAny { values, .. }
            | FilterExpr::MatchExcept { values, .. } => {
                for v in values {
                    self.value(v);
                }
            }
            FilterExpr::And { operands }
            | FilterExpr::Or { operands }
            | FilterExpr::MinShould { operands, .. } => {
                for op in operands {
                    self.filter(op);
                }
            }
            FilterExpr::Not { operand } => self.filter(operand),
            FilterExpr::Nested { filter, .. } => self.filter(filter),
            _ => {}
        }
    }

    fn formula(&mut self, expr: &FormulaExpr) {
        match expr {
            FormulaExpr::Variable { name } => self.formula_var(name),
            FormulaExpr::Sum { left, right }
            | FormulaExpr::Sub { left, right }
            | FormulaExpr::Mul { left, right }
            | FormulaExpr::Div { left, right, .. }
            | FormulaExpr::Pow {
                base: left,
                exponent: right,
            } => {
                self.formula(left);
                self.formula(right);
            }
            FormulaExpr::Neg { operand }
            | FormulaExpr::Abs { x: operand }
            | FormulaExpr::Sqrt { x: operand, .. }
            | FormulaExpr::Log { x: operand, .. }
            | FormulaExpr::Ln { x: operand, .. }
            | FormulaExpr::Exp { x: operand }
            | FormulaExpr::Acosh { x: operand, .. } => self.formula(operand),
            FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
                for arg in args {
                    self.formula(arg);
                }
            }
            FormulaExpr::Decay { x, target, .. } => {
                self.formula(x);
                if let Some(t) = target {
                    self.formula(t);
                }
            }
            FormulaExpr::Case { cond, then_, else_ } => {
                self.filter(cond);
                self.formula(then_);
                self.formula(else_);
            }
            FormulaExpr::MatchCondition { values, .. } => {
                for v in values {
                    self.value(v);
                }
            }
            _ => {}
        }
    }

    fn prefetch(&mut self, prefetch: &Prefetch) {
        if let PrefetchSource::Query(q) = &prefetch.source {
            self.query_stmt(q);
        }
        if let Some(f) = &prefetch.filter {
            self.filter(f);
        }
        if let Some(spec) = &prefetch.lookup {
            self.shard_opt(&spec.shard_key);
        }
    }

    fn query_expr(&mut self, expr: &QueryExpr) {
        match expr {
            QueryExpr::Points { ids } => {
                for id in ids {
                    self.point_id(id);
                }
            }
            QueryExpr::Nearest {
                input, prefetch, ..
            } => {
                self.query_input(input);
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::Recommend {
                positive,
                negative,
                prefetch,
                ..
            } => {
                for item in positive.iter().chain(negative.iter()) {
                    self.query_input(item);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::Context {
                pairs, prefetch, ..
            } => {
                for pair in pairs {
                    self.query_input(&pair.positive);
                    self.query_input(&pair.negative);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::Discover {
                target,
                context,
                prefetch,
                ..
            } => {
                self.query_input(target);
                for pair in context {
                    self.query_input(&pair.positive);
                    self.query_input(&pair.negative);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::OrderBy { start_from, .. } => {
                if let Some(v) = start_from {
                    self.value(v);
                }
            }
            QueryExpr::SampleRandom => {}
            QueryExpr::Fusion { prefetch, .. } => {
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::Formula {
                expression,
                defaults,
                prefetch,
            } => {
                self.formula(expression);
                for (_, v) in defaults {
                    self.value(v);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::RelevanceFeedback {
                target,
                feedback,
                prefetch,
                ..
            } => {
                self.query_input(target);
                for item in feedback {
                    self.query_input(&item.example);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::Hybrid { text_param, .. } => {
                if let Some(param) = text_param {
                    self.param_str(param, None);
                }
            }
            QueryExpr::Rerank {
                input, prefetch, ..
            } => {
                self.query_input(input);
                for p in prefetch {
                    self.prefetch(p);
                }
            }
            QueryExpr::CrossRerank {
                query_param,
                prefetch,
                ..
            } => {
                if let Some(param) = query_param {
                    self.param_str(param, None);
                }
                for p in prefetch {
                    self.prefetch(p);
                }
            }
        }
    }

    /// `PARAMS (idf = WHERE …)` corpus filter, shared by query-level `PARAMS`
    /// and `BATCH … PARAMS`.
    fn search_params(&mut self, params: Option<&crate::ast::SearchParams>) {
        if let Some(corpus) = params
            .and_then(|params| params.idf.as_ref())
            .and_then(|idf| idf.corpus.as_ref())
        {
            self.filter(corpus);
        }
    }

    fn query_stmt(&mut self, query: &QueryStmt) {
        for cte in &query.ctes {
            self.query_stmt(&cte.query);
        }
        self.query_expr(&query.expression);
        if let Some(filter) = &query.filter {
            self.filter(filter);
        }
        self.search_params(query.params.as_ref());
        self.shard_opt(&query.shard_key);
        if let Some(param) = &query.page.limit_param {
            let span = query.page.limit_span;
            if let Some(name) = param.strip_prefix(':') {
                self.record_named(name);
            } else if let Some(idx_str) = param.strip_prefix('?') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    self.record_pos(idx);
                } else {
                    self.max_pos = self.max_pos.max(1);
                }
            } else {
                self.record_named(param);
            }
            let full = unbound_param_str_err(param, span);
            let scalar = unbound_param_str_err(param, span);
            self.note_both(full, scalar);
        }
        if let Some(param) = &query.page.offset_param {
            let span = query.page.offset_span;
            if let Some(name) = param.strip_prefix(':') {
                self.record_named(name);
            } else if let Some(idx_str) = param.strip_prefix('?') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    self.record_pos(idx);
                } else {
                    self.max_pos = self.max_pos.max(1);
                }
            } else {
                self.record_named(param);
            }
            let full = unbound_param_str_err(param, span);
            let scalar = unbound_param_str_err(param, span);
            self.note_both(full, scalar);
        }
    }

    fn point_selector(&mut self, sel: &PointSelector) {
        match sel {
            PointSelector::Id(id) => self.point_id(id),
            PointSelector::Ids(ids) => {
                for id in ids {
                    self.point_id(id);
                }
            }
            PointSelector::Filter(f) => self.filter(f),
        }
    }

    fn page_limit_param(&mut self, param: &str, span: Option<Span>) {
        self.param_str(param, span);
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Query(query) => self.query_stmt(query),
            Stmt::Scroll(scroll) => {
                if let Some(filter) = &scroll.filter {
                    self.filter(filter);
                }
                if let Some(after) = &scroll.after {
                    self.point_id(after);
                }
                if let Some(order) = &scroll.order_by
                    && let Some(v) = &order.start_from
                {
                    self.value(v);
                }
                if let Some(param) = &scroll.limit_param {
                    self.page_limit_param(param, scroll.limit_span);
                }
                self.shard_opt(&scroll.shard_key);
            }
            Stmt::Upsert(upsert) => {
                for point in &upsert.points {
                    match point {
                        crate::ast::PointEntry::Inline(inline) => {
                            self.point_id(&inline.id);
                            if let Some(vectors) = &inline.vectors {
                                self.point_vectors(vectors);
                            }
                            for (_, v) in &inline.payload {
                                self.value(v);
                            }
                        }
                        crate::ast::PointEntry::Param(name, span) => {
                            self.record_named(name);
                            self.has_point_params = true;
                            self.note_full(unbound_named_err(name, span.as_deref().copied()));
                        }
                        crate::ast::PointEntry::PositionalParam(idx, span) => {
                            self.record_pos(*idx);
                            self.has_point_params = true;
                            self.note_full(unbound_positional_err(*idx, span.as_deref().copied()));
                        }
                    }
                }
                if let Some(filter) = &upsert.update_filter {
                    self.filter(filter);
                }
                self.shard_opt(&upsert.shard_key);
            }
            Stmt::Delete(del) => {
                self.point_selector(&del.selector);
                self.shard_opt(&del.shard_key);
            }
            Stmt::ClearPayload(cp) => {
                self.point_selector(&cp.selector);
                self.shard_opt(&cp.shard_key);
            }
            Stmt::DeletePayload(dp) => {
                self.point_selector(&dp.selector);
                self.shard_opt(&dp.shard_key);
            }
            Stmt::DeleteVector(dv) => {
                self.point_selector(&dv.selector);
                self.shard_opt(&dv.shard_key);
            }
            Stmt::UpdateVector(uv) => {
                for point in &uv.points {
                    self.point_id(&point.id);
                    self.point_vectors(&point.vectors);
                }
                self.shard_opt(&uv.shard_key);
            }
            Stmt::UpdatePayload(up) => {
                self.point_selector(&up.selector);
                for (_, v) in &up.payload {
                    self.value(v);
                }
                self.shard_opt(&up.shard_key);
            }
            Stmt::Count(count) => {
                if let Some(filter) = &count.filter {
                    self.filter(filter);
                }
                self.shard_opt(&count.shard_key);
            }
            Stmt::Facet(facet) => {
                if let Some(filter) = &facet.filter {
                    self.filter(filter);
                }
                if let Some(param) = &facet.limit_param {
                    self.page_limit_param(param, facet.limit_span);
                }
                self.shard_opt(&facet.shard_key);
            }
            Stmt::CreateShardKey(sk) => self.shard_opt(&Some(sk.shard_key.clone())),
            Stmt::DropShardKey(sk) => self.shard_opt(&Some(sk.shard_key.clone())),
            Stmt::CreateCollection(cc) => {
                self.shard_list(
                    cc.config
                        .as_ref()
                        .and_then(|config| config.params.as_ref())
                        .and_then(|params| params.shard_keys.as_ref()),
                );
                if let Some(config) = &cc.config {
                    self.ddl_options(config);
                }
            }
            Stmt::AlterCollection(ac) => {
                self.shard_list(
                    ac.config
                        .as_ref()
                        .and_then(|config| config.params.as_ref())
                        .and_then(|params| params.shard_keys.as_ref()),
                );
                if let Some(config) = &ac.config {
                    self.ddl_options(config);
                }
            }
            Stmt::Batch(batch) => {
                self.search_params(batch.params.as_ref());
                for member in &batch.statements {
                    self.stmt(member);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn run_census(stmt: &Stmt) -> Census {
    let mut census = Census::new();
    census.stmt(stmt);
    census
}

/// Collect all named parameter names and the maximum positional parameter
/// index present anywhere in a statement AST.
pub fn collect_statement_params(
    stmt: &Stmt,
) -> (alloc::collections::BTreeSet<alloc::string::String>, usize) {
    let census = run_census(stmt);
    (census.named, census.max_pos)
}

/// Whether an upsert template carries whole-point placeholders (`VALUES :p`
/// / `VALUES ?`) needing dict splice at execution time.
pub fn stmt_has_point_params(stmt: &Stmt) -> bool {
    run_census(stmt).has_point_params
}
