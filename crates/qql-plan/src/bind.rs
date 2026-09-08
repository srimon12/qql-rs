//! Bind vector parameters into a planned operation without re-planning.

use crate::plan::PlannedOperation;
use crate::types::*;

impl PlannedOperation {
    /// Bind vector parameters directly into this planned operation without re-planning.
    pub fn bind_vector_params<F, P>(&mut self, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        match self {
            PlannedOperation::Query { request, .. } => {
                Self::bind_query_request_vectors(request, named, positional);
            }
            PlannedOperation::Upsert { request, .. } => {
                for p in &mut request.points {
                    if let Some(ref mut pvs) = p.vector {
                        Self::bind_point_vectors_val(pvs, named, positional);
                    }
                }
            }
            PlannedOperation::UpdateVectors { request, .. } => {
                for p in &mut request.points {
                    Self::bind_point_vectors_val(&mut p.vector, named, positional);
                }
            }
            _ => {}
        }
    }

    fn bind_point_vectors_val<F, P>(pvs: &mut PlanPointVectors, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        match pvs {
            PlanPointVectors::Param(name) => {
                if let Some(new_v) = named(name) {
                    *pvs = PlanPointVectors::Unnamed(new_v);
                }
            }
            PlanPointVectors::PositionalParam(idx) => {
                if let Some(new_v) = positional(*idx) {
                    *pvs = PlanPointVectors::Unnamed(new_v);
                }
            }
            PlanPointVectors::Unnamed(v) => {
                Self::bind_vector_val(v, named, positional);
            }
            PlanPointVectors::Named(entries) => {
                for (_, v) in entries {
                    Self::bind_vector_val(v, named, positional);
                }
            }
        }
    }

    fn bind_query_request_vectors<F, P>(req: &mut QueryRequest, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        Self::bind_query_variant_vectors(&mut req.query, named, positional);
        for prefetch in &mut req.prefetch {
            if let Some(ref mut qv) = prefetch.query {
                Self::bind_query_variant_vectors(qv, named, positional);
            }
        }
    }

    fn bind_plan_query_input<F, P>(input: &mut PlanQueryInput, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        if let PlanQueryInput::Vector(v) = input {
            Self::bind_vector_val(v, named, positional);
        }
    }

    fn bind_query_variant_vectors<F, P>(qv: &mut QueryVariant, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        match qv {
            QueryVariant::Nearest(nearest) => {
                Self::bind_plan_query_input(&mut nearest.nearest, named, positional);
            }
            QueryVariant::Recommend { recommend } => {
                for pos in &mut recommend.positive {
                    Self::bind_plan_query_input(pos, named, positional);
                }
                for neg in &mut recommend.negative {
                    Self::bind_plan_query_input(neg, named, positional);
                }
            }
            QueryVariant::Context { context } => {
                for pair in context {
                    Self::bind_plan_query_input(&mut pair.positive, named, positional);
                    Self::bind_plan_query_input(&mut pair.negative, named, positional);
                }
            }
            QueryVariant::Discover { discover } => {
                Self::bind_plan_query_input(&mut discover.target, named, positional);
                for pair in &mut discover.context {
                    Self::bind_plan_query_input(&mut pair.positive, named, positional);
                    Self::bind_plan_query_input(&mut pair.negative, named, positional);
                }
            }
            QueryVariant::RelevanceFeedback { relevance_feedback } => {
                Self::bind_plan_query_input(&mut relevance_feedback.target, named, positional);
                for fb in &mut relevance_feedback.feedback {
                    Self::bind_plan_query_input(&mut fb.example, named, positional);
                }
            }
            _ => {}
        }
    }

    fn bind_vector_val<F, P>(v: &mut PlanVectorValue, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        match v {
            PlanVectorValue::Param(name) => {
                if let Some(new_v) = named(name) {
                    *v = new_v;
                }
            }
            PlanVectorValue::PositionalParam(idx) => {
                if let Some(new_v) = positional(*idx) {
                    *v = new_v;
                }
            }
            _ => {}
        }
    }
}
