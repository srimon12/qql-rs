//! Plan-time validation that every point ID carried by a plan is a real
//! Qdrant point ID: an unsigned integer or a UUID.
//!
//! String IDs stay opaque inside [`crate::semantic::PlanPointId`] so response
//! parsing can carry server values verbatim and conversions stay infallible.
//! The planner therefore walks the finished [`PlannedOperation`] once, after
//! all lowering. Without that walk a non-UUID string reaches each transport
//! differently: Qdrant answers 400 on REST, the gRPC layer stuffs the string
//! into the `uuid` oneof verbatim, and edge rejects it with its own transport
//! code. One `QQL-PLAN-POINT-ID` here replaces all three outcomes.

use alloc::format;

use crate::plan::PlannedOperation;
use crate::semantic::{PlanPointId, PlanQueryInput};
use crate::types::*;
use qql_core::error::QqlError;

/// Stable code for a string point ID that is not a UUID.
const POINT_ID_CODE: &str = "QQL-PLAN-POINT-ID";

/// Validate one point ID; numbers always pass.
fn validate_id(id: &PlanPointId) -> Result<(), QqlError> {
    match id {
        PlanPointId::Number(_) => Ok(()),
        PlanPointId::String(s) if uuid::Uuid::parse_str(s).is_ok() => Ok(()),
        PlanPointId::String(s) => Err(QqlError::validation(
            POINT_ID_CODE,
            format!("point ID \"{s}\" is not a UUID; point IDs must be unsigned integers or UUIDs"),
            None,
        )),
    }
}

fn validate_ids(ids: &[PlanPointId]) -> Result<(), QqlError> {
    ids.iter().try_for_each(validate_id)
}

fn validate_points_and_filter(
    points: &Option<Vec<PlanPointId>>,
    filter: Option<&FilterExpression>,
) -> Result<(), QqlError> {
    if let Some(points) = points {
        validate_ids(points)?;
    }
    if let Some(filter) = filter {
        validate_filter(filter)?;
    }
    Ok(())
}

fn validate_filter(filter: &FilterExpression) -> Result<(), QqlError> {
    match filter {
        FilterExpression::Single(clause) => validate_clause(clause),
        FilterExpression::Compound(compound) => validate_compound(compound),
    }
}

fn validate_compound(compound: &FilterCompound) -> Result<(), QqlError> {
    compound
        .must
        .iter()
        .chain(&compound.must_not)
        .chain(&compound.should)
        .try_for_each(validate_clause)?;
    if let Some(min_should) = &compound.min_should {
        min_should.conditions.iter().try_for_each(validate_clause)?;
    }
    Ok(())
}

fn validate_clause(clause: &FilterClause) -> Result<(), QqlError> {
    match clause {
        FilterClause::HasId(has_id) => validate_ids(&has_id.has_id),
        FilterClause::MinShould(min_should) => min_should
            .min_should
            .conditions
            .iter()
            .try_for_each(validate_clause),
        FilterClause::Nested(nested) => validate_filter(&nested.nested.filter),
        FilterClause::Filter(compound) => validate_compound(compound),
        _ => Ok(()),
    }
}

fn validate_input(input: &PlanQueryInput) -> Result<(), QqlError> {
    match input {
        PlanQueryInput::Point(id) => validate_id(id),
        _ => Ok(()),
    }
}

fn validate_context_pairs(pairs: &[ContextPair]) -> Result<(), QqlError> {
    for pair in pairs {
        validate_input(&pair.positive)?;
        validate_input(&pair.negative)?;
    }
    Ok(())
}

fn validate_variant(variant: &QueryVariant) -> Result<(), QqlError> {
    match variant {
        QueryVariant::Nearest(nearest) => validate_input(&nearest.nearest),
        QueryVariant::Recommend { recommend } => {
            recommend.positive.iter().try_for_each(validate_input)?;
            recommend.negative.iter().try_for_each(validate_input)
        }
        QueryVariant::Context { context } => validate_context_pairs(context),
        QueryVariant::Discover { discover } => {
            validate_input(&discover.target)?;
            validate_context_pairs(&discover.context)
        }
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            validate_input(&relevance_feedback.target)?;
            relevance_feedback
                .feedback
                .iter()
                .try_for_each(|item| validate_input(&item.example))
        }
        _ => Ok(()),
    }
}

fn validate_prefetch(prefetch: &PrefetchRequest) -> Result<(), QqlError> {
    if let Some(query) = &prefetch.query {
        validate_variant(query)?;
    }
    if let Some(filter) = &prefetch.filter {
        validate_filter(filter)?;
    }
    if let Some(nested) = &prefetch.prefetch {
        nested.iter().try_for_each(validate_prefetch)?;
    }
    Ok(())
}

fn validate_query_parts(
    query: &QueryVariant,
    prefetch: &[PrefetchRequest],
    filter: Option<&FilterExpression>,
) -> Result<(), QqlError> {
    validate_variant(query)?;
    prefetch.iter().try_for_each(validate_prefetch)?;
    if let Some(filter) = filter {
        validate_filter(filter)?;
    }
    Ok(())
}

/// Validate every point ID in a planned operation, recursing into batch
/// members and prefetch stages.
pub(crate) fn validate_planned_point_ids(op: &PlannedOperation) -> Result<(), QqlError> {
    match op {
        PlannedOperation::Query { request, .. } => {
            validate_query_parts(&request.query, &request.prefetch, request.filter.as_ref())
        }
        PlannedOperation::QueryGroups { request, .. } => {
            validate_query_parts(&request.query, &request.prefetch, request.filter.as_ref())
        }
        PlannedOperation::GetPoints { request, .. } => validate_ids(&request.ids),
        PlannedOperation::Scroll { request, .. } => {
            validate_points_and_filter(&None, request.filter.as_ref())?;
            if let Some(offset) = &request.offset {
                validate_id(offset)?;
            }
            Ok(())
        }
        PlannedOperation::Count { request, .. } => {
            validate_points_and_filter(&None, request.filter.as_ref())
        }
        PlannedOperation::Facet { request, .. } => {
            validate_points_and_filter(&None, request.filter.as_ref())
        }
        PlannedOperation::Upsert { request, .. } => {
            request
                .points
                .iter()
                .try_for_each(|point| validate_id(&point.id))?;
            validate_points_and_filter(&None, request.update_filter.as_ref())
        }
        PlannedOperation::Delete { request, .. } => {
            validate_points_and_filter(&request.points, request.filter.as_ref())
        }
        PlannedOperation::UpdatePayload { request, .. }
        | PlannedOperation::OverwritePayload { request, .. } => {
            validate_points_and_filter(&request.points, request.filter.as_ref())
        }
        PlannedOperation::ClearPayload { request, .. } => {
            validate_points_and_filter(&request.points, request.filter.as_ref())
        }
        PlannedOperation::DeletePayload { request, .. } => {
            validate_points_and_filter(&request.points, request.filter.as_ref())
        }
        PlannedOperation::UpdateVectors { request, .. } => request
            .points
            .iter()
            .try_for_each(|point| validate_id(&point.id)),
        PlannedOperation::DeleteVectors { request, .. } => {
            validate_points_and_filter(&request.points, request.filter.as_ref())
        }
        PlannedOperation::CrossRerank { candidates, .. } => {
            candidates.iter().try_for_each(|(_, request)| {
                validate_query_parts(&request.query, &request.prefetch, request.filter.as_ref())
            })
        }
        PlannedOperation::Batch { operations, .. } => {
            operations.iter().try_for_each(validate_planned_point_ids)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use qql_core::error::QqlError;
    use qql_core::parser::Parser;

    fn plan_err(source: &str) -> QqlError {
        let stmt = Parser::parse(source).unwrap();
        crate::plan::plan(&stmt).unwrap_err()
    }

    #[test]
    fn rejects_non_uuid_string_point_ids_at_plan_time() {
        let cases = [
            "QUERY POINTS (1, 2, 'uuid-3') FROM docs;",
            "QUERY NEAREST POINT 'uuid-3' FROM docs USING dense;",
            "QUERY NEAREST VECTOR [0.1] FROM docs WHERE id = 'uuid-3' LIMIT 5;",
            "QUERY RECOMMEND POSITIVE ('uuid-3') NEGATIVE (1) FROM docs USING dense;",
            "UPSERT INTO docs VALUES {id: 'uuid-3', title: 'x'};",
            "DELETE FROM docs WHERE id = 'uuid-3';",
            "UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 'uuid-3';",
            "SCROLL FROM docs AFTER 'uuid-3' LIMIT 5;",
            "BATCH { QUERY NEAREST POINT 'uuid-3' FROM docs USING dense; QUERY NEAREST POINT 1 FROM docs USING dense; }",
        ];
        for source in cases {
            assert_eq!(plan_err(source).code, "QQL-PLAN-POINT-ID", "{source}");
        }
    }

    #[test]
    fn accepts_uuid_and_numeric_point_ids() {
        let stmt =
            Parser::parse("QUERY POINTS (1, '550e8400-e29b-41d4-a716-446655440000') FROM docs;")
                .unwrap();
        crate::plan::plan(&stmt).expect("valid point IDs must plan");
    }
}
