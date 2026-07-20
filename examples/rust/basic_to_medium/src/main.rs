use std::borrow::Cow;
use qql_core::ast::{self, Value};
use qql_core::parser::Parser;

fn main() {
    let q = "QUERY 'machine learning transformer' FROM papers LIMIT 20";

    let mut s = Parser::parse(q).unwrap();
    ast::inject_filter(&mut s, "tenant_id", "=", &Value::Str(Cow::Borrowed("acme-corp")));
    println!("=== String filter ===");
    println!("{:#?}", s);

    let mut s = Parser::parse(q).unwrap();
    ast::inject_filter(&mut s, "impact_factor", ">=", &Value::Float(5.0));
    println!("\n=== Numeric filter ===");
    println!("{:#?}", s);

    let mut s = Parser::parse(q).unwrap();
    ast::inject_filter(&mut s, "is_published", "=", &Value::Bool(true));
    println!("\n=== Boolean filter ===");
    println!("{:#?}", s);
}
