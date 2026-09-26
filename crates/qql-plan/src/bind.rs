//! Bind vector parameters into a planned operation without re-planning.

use crate::plan::PlannedOperation;
use crate::types::*;

impl PlannedOperation {
    /// Bind vector parameters directly into this planned operation without re-planning.
    ///
    /// Covers every operation whose wire form can carry an unbound
    /// vector parameter: `QUERY` (including nested prefetch stages),
    /// `QUERY … GROUP BY`, `UPSERT`, `UPDATE … VECTOR`, explicit `BATCH`
    /// members, and client-side `CROSS RERANK` candidates. Operations with no
    /// vector input (`SCROLL`, `COUNT`, `FACET`, DDL, SHOW, …) fall through to
    /// the no-op arm.
    ///
    /// A member left unbound (lookup returned `None`) keeps its placeholder,
    /// which the boundary renderers reject loudly instead of shipping it.
    pub fn bind_vector_params<F, P>(&mut self, named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        match self {
            PlannedOperation::Query { request, .. } => {
                Self::bind_query_request_vectors(request, named, positional);
            }
            PlannedOperation::QueryGroups { request, .. } => {
                Self::bind_query_variant_vectors(&mut request.query, named, positional);
                Self::bind_prefetch_vectors(&mut request.prefetch, named, positional);
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
            PlannedOperation::Batch { operations, .. } => {
                for operation in operations {
                    operation.bind_vector_params(named, positional);
                }
            }
            PlannedOperation::CrossRerank { candidates, .. } => {
                for (_, request) in candidates {
                    Self::bind_query_request_vectors(request, named, positional);
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
        Self::bind_prefetch_vectors(&mut req.prefetch, named, positional);
    }

    /// Bind vectors in a prefetch stage list, recursing into nested stages.
    fn bind_prefetch_vectors<F, P>(prefetches: &mut [PrefetchRequest], named: &F, positional: &P)
    where
        F: Fn(&str) -> Option<PlanVectorValue>,
        P: Fn(usize) -> Option<PlanVectorValue>,
    {
        for prefetch in prefetches {
            if let Some(query) = &mut prefetch.query {
                Self::bind_query_variant_vectors(query, named, positional);
            }
            if let Some(nested) = &mut prefetch.prefetch {
                Self::bind_prefetch_vectors(nested, named, positional);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::plan_template;
    use qql_core::parser::Parser;

    fn dense(values: &[f32]) -> PlanVectorValue {
        PlanVectorValue::Dense(values.to_vec())
    }

    /// The vector bound into a query variant's primary input.
    fn variant_vector(variant: &QueryVariant) -> Option<&PlanVectorValue> {
        match variant {
            QueryVariant::Nearest(nearest) => match &nearest.nearest {
                PlanQueryInput::Vector(value) => Some(value),
                _ => None,
            },
            _ => None,
        }
    }

    #[test]
    fn binds_query_groups_vectors() {
        let stmt = Parser::parse("QUERY NEAREST :v FROM docs USING dense GROUP BY topic LIMIT 5;")
            .expect("template parses");
        let mut op = plan_template(&stmt).expect("template plans");
        op.bind_vector_params(&|name| (name == "v").then(|| dense(&[0.1, 0.2])), &|_| None);
        let PlannedOperation::QueryGroups { request, .. } = &op else {
            panic!("expected QueryGroups, got {op:?}");
        };
        assert_eq!(variant_vector(&request.query), Some(&dense(&[0.1, 0.2])));
    }

    #[test]
    fn binds_nested_prefetch_vectors_two_levels_deep() {
        let stmt = Parser::parse(
            "QUERY NEAREST :v FROM docs USING dense \
             PREFETCH (QUERY NEAREST :w FROM docs USING dense \
               PREFETCH (QUERY NEAREST :u FROM docs USING dense LIMIT 5) LIMIT 10) LIMIT 3;",
        )
        .expect("template parses");
        let mut op = plan_template(&stmt).expect("template plans");
        op.bind_vector_params(
            &|name| match name {
                "v" => Some(dense(&[1.0])),
                "w" => Some(dense(&[2.0])),
                "u" => Some(dense(&[3.0])),
                _ => None,
            },
            &|_| None,
        );
        let PlannedOperation::Query { request, .. } = &op else {
            panic!("expected Query, got {op:?}");
        };
        assert_eq!(variant_vector(&request.query), Some(&dense(&[1.0])));
        let level_one = request.prefetch[0].query.as_ref().expect("level-1 query");
        assert_eq!(variant_vector(level_one), Some(&dense(&[2.0])));
        let level_two = request.prefetch[0].prefetch.as_ref().expect("level-2 list")[0]
            .query
            .as_ref()
            .expect("level-2 query");
        assert_eq!(variant_vector(level_two), Some(&dense(&[3.0])));
    }

    #[test]
    fn binds_cross_rerank_candidate_vectors() {
        let stmt = Parser::parse(
            "QUERY CROSS RERANK TEXT 'q' MODEL 'bge' ON FIELD body FROM docs \
             PREFETCH (QUERY NEAREST :v FROM docs USING dense LIMIT 5) LIMIT 10;",
        )
        .expect("template parses");
        let mut op = plan_template(&stmt).expect("template plans");
        op.bind_vector_params(&|name| (name == "v").then(|| dense(&[0.5])), &|_| None);
        let PlannedOperation::CrossRerank { candidates, .. } = &op else {
            panic!("expected CrossRerank, got {op:?}");
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(variant_vector(&candidates[0].1.query), Some(&dense(&[0.5])));
    }

    #[test]
    fn binds_positional_vectors_inside_batch_members() {
        // Explicit BATCH members carry full query requests; positional
        // placeholders must bind inside them too.
        let stmt = Parser::parse(
            "BATCH { QUERY VECTOR ? FROM docs USING dense LIMIT 1; \
               QUERY VECTOR ? FROM docs USING dense LIMIT 1; }",
        )
        .expect("template parses");
        let mut op = plan_template(&stmt).expect("template plans");
        op.bind_vector_params(&|_| None, &|idx| match idx {
            0 => Some(dense(&[7.0])),
            1 => Some(dense(&[8.0])),
            _ => None,
        });
        let PlannedOperation::Batch { operations, .. } = &op else {
            panic!("expected Batch, got {op:?}");
        };
        assert_eq!(operations.len(), 2);
        let PlannedOperation::Query { request, .. } = &operations[0] else {
            panic!("expected member Query");
        };
        assert_eq!(variant_vector(&request.query), Some(&dense(&[7.0])));
        let PlannedOperation::Query { request, .. } = &operations[1] else {
            panic!("expected member Query");
        };
        assert_eq!(variant_vector(&request.query), Some(&dense(&[8.0])));
    }
}
