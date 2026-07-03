mod equality;
mod logical;
mod sets_matching;

use crate::filter_conv::*;
use qql_core::ast::FilterExpr;

fn build(expr: &FilterExpr) -> QdrantFilter {
    FilterConverter.build_filter(expr).unwrap().unwrap()
}
