# qql-core

Standalone Lexer, Parser, AST representation, and Query Transformers for the Qdrant Query Language (QQL).

---

## Features
* **Zero I/O Dependencies**: A pure computation library with no network or system call overhead.
* **Standard Compatibility**: Fully compatible with standard environments, enabling compilation to WebAssembly (WASM) and lightweight edge systems.
* **Lexer**: Tokenizes raw query strings into structured lexer tokens.
* **Parser**: Generates a typed Abstract Syntax Tree (AST) representing QQL queries.
* **AST Transformations**: Provides standard AST mutation utilities (such as `inject_filter` for recursive tenant-isolation and query security injection).

---

## Installation

Add `qql-core` to your `Cargo.toml`:
```toml
[dependencies]
qql-core = { path = "../qql-core" }
```

---

## Basic Usage

### Parsing a QQL Statement
```rust
use qql_core::parser::parse;

fn main() {
    let query_str = "QUERY 'vector search' FROM articles LIMIT 10";
    let stmt = parse(query_str).expect("Failed to parse query");
    println!("{:#?}", stmt);
}
```

### Injecting Security Filters
```rust
use qql_core::ast::{Value, Stmt};
use qql_core::ast::inject_filter;
use qql_core::parser::parse;

fn main() {
    let mut stmt = parse("QUERY 'vector search' FROM articles LIMIT 10").unwrap();

    // Inject org_id = 'acme-corp' recursively into the main statement and any nested subqueries/CTEs
    inject_filter(&mut stmt, "org_id", "=", &Value::Str("acme-corp"));
}
```
