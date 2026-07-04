mod equality;
mod logical;
mod sets_matching;

use crate::filter_conv::*;
use qql_core::ast::FilterExpr;

fn build(expr: &FilterExpr) -> serde_json::Value {
    let filter = FilterConverter.build_filter(expr).unwrap().unwrap();
    serde_json::to_value(&filter).unwrap()
}
