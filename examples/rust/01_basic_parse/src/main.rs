use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

fn main() {
    // Parse a CREATE COLLECTION statement
    let stmt = Parser::parse("CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)").unwrap();
    println!("=== Parsed AST ===");
    println!("{:#}", &format!("{:#?}", stmt)[..500.min(format!("{:#?}", stmt).len())]);
    println!();

    // Tokenize a QUERY
    let lexer = Lexer::new("QUERY 'vector database' FROM docs LIMIT 10");
    println!("=== Tokens ===");
    for tok in lexer {
        let tok = tok.unwrap();
        println!("  {:12} {:30}  pos={}", tok.kind.as_str(), tok.text, tok.pos);
    }
    println!();

    // Validate queries
    for q in [
        "QUERY 'hello' FROM docs LIMIT 5",
        "CREATE COLLECTION docs",
        "SELECT * FROM docs WHERE id = 1",
        "",
        "BOGUS STUFF",
    ] {
        let valid = Parser::parse(q).is_ok();
        println!("  valid={:<5}  {:?}", valid, q);
    }
}
