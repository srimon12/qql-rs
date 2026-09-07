//! Filter expression parameter binding.

use super::value::{bind_point_id, bind_value};
use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::statement::PointSelector;
use crate::error::QqlError;

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

/// Bind parameters into a `PointSelector` in-place.
pub fn bind_point_selector<F>(
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
