use std::borrow::Cow;
use qql_core::ast::{self, Value};
use qql_core::parser::Parser;

fn main() {
    let user_query = "QUERY 'machine learning transformer' FROM papers LIMIT 20";

    // Tenant isolation — inject a string filter
    let mut stmt = Parser::parse(user_query).unwrap();
    ast::inject_filter(&mut stmt, "tenant_id", "=", &Value::Str(Cow::Borrowed("acme-corp")));
    println!("=== Tenant isolation ===");
    println!("{:#?}\n", stmt);

    // Multi-tenant + access control — inject TWO filters
    let mut stmt = Parser::parse(user_query).unwrap();
    ast::inject_filter(&mut stmt, "tenant_id", "=", &Value::Str(Cow::Borrowed("acme-corp")));
    ast::inject_filter(
        &mut stmt,
        "visibility",
        "IN",
        &Value::List(vec![
            Value::Str(Cow::Borrowed("public")),
            Value::Str(Cow::Borrowed("internal")),
        ]),
    );
    println!("=== Tenant + access control ===");
    println!("{:#?}\n", stmt);

    // Numeric filter — inject a float comparison
    let mut stmt = Parser::parse("QUERY 'covid research' FROM publications LIMIT 10").unwrap();
    ast::inject_filter(&mut stmt, "impact_factor", ">=", &Value::Float(5.0));
    println!("=== Numeric filter ===");
    println!("{:#?}\n", stmt);
}
