use qql_core::parser::Parser;

fn main() {
    let script = "
        CREATE COLLECTION docs HYBRID;
        INSERT INTO docs VALUES {id: 1, text: 'first'};
        INSERT INTO docs VALUES {id: 2, text: 'second'};
        QUERY 'test' FROM docs LIMIT 10;
    ";
    let stmts = Parser::parse_all(script).unwrap();
    println!("=== Script Parsing (parse_all) ===");
    println!("Parsed {} statements from a .qql script:", stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        println!("  [{}] {:?}", i, s);
    }

    let queries = [
        "QUERY 'alpha' FROM docs LIMIT 5",
        "QUERY 'beta'  FROM docs LIMIT 5",
        "QUERY 'gamma' FROM docs LIMIT 5",
    ];
    println!("\n=== Native Loop Parsing ===");
    println!("Parsed {} queries:", queries.len());
    for (i, q) in queries.iter().enumerate() {
        let stmt = Parser::parse(q).unwrap();
        println!("  [{}] {:?}", i, stmt);
    }
}
