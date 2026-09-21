//! Filter expression core: entry points, clause dispatch, compound nesting.
//!
//! Pure move from `filter_converter.rs` (size hygiene split). Match/range
//! lowering lives in `super::matching`, geo/slice lowering in `super::geo`.

use qdrant_edge::{
    Condition, FieldCondition, Filter, HasIdCondition, HasVectorCondition, IsEmptyCondition,
    IsNullCondition, MinShould, Nested, NestedCondition,
};
use qql_core::error::QqlError;
use qql_plan::types::{
    FieldCondition as PlanFieldCondition, FilterClause as PlanFilterClause,
    FilterCompound as PlanFilterCompound, FilterExpression as PlanFilterExpression,
    NestedCondition as PlanNestedCondition,
};

use super::geo::{lower_geo_bounding_box, lower_geo_polygon, lower_geo_radius, lower_slice};
use super::lower_key;
use super::matching::{lower_match, lower_range, lower_values_count};
use crate::backend::conversions::to_edge_id;

/// Lower a plan filter into an in-process `qdrant_edge::Filter`.
///
/// The single filter converter for every edge path — legacy JSON envelopes and
/// typed responses both call this, so both observe identical filter semantics.
pub(crate) fn convert_edge_filter(
    filter: Option<&PlanFilterExpression>,
) -> Result<Option<Filter>, QqlError> {
    match filter {
        None => Ok(None),
        Some(expression) => Ok(Some(lower_expression(expression)?)),
    }
}

fn lower_expression(expression: &PlanFilterExpression) -> Result<Filter, QqlError> {
    match expression {
        // A bare `Condition` is not a `Filter` on the wire; wrapping it in
        // `must` preserves the old serde-shim semantics for every clause kind.
        PlanFilterExpression::Single(clause) => Ok(Filter {
            must: Some(vec![lower_clause(clause)?]),
            ..Filter::default()
        }),
        PlanFilterExpression::Compound(compound) => lower_compound(compound),
    }
}

/// Lower a single condition term used by a formula `CASE` / `MATCH` expression.
///
/// Unlike [`convert_edge_filter`], the result stays a bare [`Condition`]:
/// qdrant-edge's `Expression::Condition` carries one condition, not a filter
/// envelope. Compound conditions (`AND` / `OR` / `NOT`) wrap in
/// `Condition::Filter`.
pub(crate) fn convert_formula_condition(
    expression: &PlanFilterExpression,
) -> Result<Condition, QqlError> {
    match expression {
        PlanFilterExpression::Single(clause) => lower_clause(clause),
        PlanFilterExpression::Compound(compound) => {
            Ok(Condition::Filter(lower_compound(compound)?))
        }
    }
}

fn lower_compound(compound: &PlanFilterCompound) -> Result<Filter, QqlError> {
    let min_should = compound
        .min_should
        .as_ref()
        .map(|min_should| {
            min_should
                .conditions
                .iter()
                .map(lower_clause)
                .collect::<Result<Vec<_>, _>>()
                .map(|conditions| MinShould {
                    conditions,
                    min_count: min_should.min_count as usize,
                })
        })
        .transpose()?;
    Ok(Filter {
        should: lower_conditions(&compound.should)?,
        min_should,
        must: lower_conditions(&compound.must)?,
        must_not: lower_conditions(&compound.must_not)?,
    })
}

/// Lower a clause list, collapsing `[]` to `None` to match the wire
/// representation (`skip_serializing_if` drops empty lists).
fn lower_conditions(clauses: &[PlanFilterClause]) -> Result<Option<Vec<Condition>>, QqlError> {
    if clauses.is_empty() {
        return Ok(None);
    }
    clauses
        .iter()
        .map(lower_clause)
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn lower_clause(clause: &PlanFilterClause) -> Result<Condition, QqlError> {
    Ok(match clause {
        PlanFilterClause::Field(condition) => Condition::Field(lower_field(condition)?),
        PlanFilterClause::IsNull(condition) => {
            Condition::IsNull(IsNullCondition::from(lower_key(&condition.is_null.key)?))
        }
        PlanFilterClause::IsEmpty(condition) => {
            Condition::IsEmpty(IsEmptyCondition::from(lower_key(&condition.is_empty.key)?))
        }
        PlanFilterClause::HasId(condition) => {
            let ids = condition
                .has_id
                .iter()
                .map(to_edge_id)
                .collect::<Result<Vec<_>, _>>()?;
            Condition::HasId(ids.into_iter().collect::<HasIdCondition>())
        }
        PlanFilterClause::HasVector(condition) => Condition::HasVector(HasVectorCondition {
            has_vector: condition.has_vector.clone(),
        }),
        PlanFilterClause::MinShould(condition) => {
            let conditions = condition
                .min_should
                .conditions
                .iter()
                .map(lower_clause)
                .collect::<Result<Vec<_>, _>>()?;
            Condition::Filter(Filter {
                min_should: Some(MinShould {
                    conditions,
                    min_count: condition.min_should.min_count as usize,
                }),
                ..Filter::default()
            })
        }
        PlanFilterClause::Nested(condition) => Condition::Nested(lower_nested(condition)?),
        PlanFilterClause::Filter(compound) => Condition::Filter(lower_compound(compound)?),
        PlanFilterClause::Slice(condition) => Condition::Slice(lower_slice(condition)?),
    })
}

fn lower_field(condition: &PlanFieldCondition) -> Result<FieldCondition, QqlError> {
    Ok(FieldCondition {
        key: lower_key(&condition.key)?,
        r#match: condition.r#match.as_ref().map(lower_match).transpose()?,
        range: condition.range.as_ref().map(lower_range).transpose()?,
        geo_bounding_box: condition
            .geo_bounding_box
            .as_ref()
            .map(lower_geo_bounding_box)
            .transpose()?,
        geo_radius: condition
            .geo_radius
            .as_ref()
            .map(lower_geo_radius)
            .transpose()?,
        geo_polygon: condition
            .geo_polygon
            .as_ref()
            .map(lower_geo_polygon)
            .transpose()?,
        values_count: condition
            .values_count
            .as_ref()
            .map(lower_values_count)
            .transpose()?,
        is_empty: condition.is_empty,
        is_null: condition.is_null,
    })
}

fn lower_nested(condition: &PlanNestedCondition) -> Result<NestedCondition, QqlError> {
    Ok(NestedCondition {
        nested: Nested {
            key: lower_key(&condition.nested.key)?,
            filter: lower_expression(&condition.nested.filter)?,
        },
    })
}
