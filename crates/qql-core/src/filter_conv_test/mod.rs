mod equality;
mod logical;
mod sets_matching;

use crate::ast::FilterExpr;
use crate::filter_conv::FilterConverter;

fn build(expr: &FilterExpr) -> serde_json::Value {
    FilterConverter::new().build_filter(expr).unwrap().unwrap()
}
