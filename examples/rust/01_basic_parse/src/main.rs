use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

fn main() {
    let stmt = Parser::parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)").unwrap();
    println!("=== Parsed AST ===");
    println!("{}", &format!("{:#?}", stmt)[..500.min(format!("{:#?}", stmt).len())]);

    println!("\n=== Tokens ===");
    for tok in Lexer::new("QUERY 'vector database' FROM docs LIMIT 10") {
        let tok = tok.unwrap();
        println!("  {:12} {:30}  pos={}", tok.kind.as_str(), tok.text, tok.pos);
    }

    println!("\n=== Validation ===");
    for q in [
        "QUERY 'hello' FROM docs LIMIT 5",
        "CREATE COLLECTION docs",
        "SELECT * FROM docs WHERE id = 1",
        "",
        "BOGUS STUFF",
    ] {
        println!("  valid={:<5}  {:?}", Parser::parse(q).is_ok(), q);
    }
}
