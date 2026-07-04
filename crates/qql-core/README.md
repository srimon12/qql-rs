# qql-core

Standalone lexer, parser, AST, and query transformers for Qdrant Query Language (QQL). Pure computation — no I/O, no network, no system calls.

## Statement Types

The parser produces a typed AST covering all QQL operations:

| Statement | Description |
|---|---|
| `SHOW COLLECTIONS` | List all collections |
| `SHOW COLLECTION name` | Describe a collection |
| `CREATE COLLECTION ...` | Create with vectors, sparse, quantization, HNSW, optimizers |
| `ALTER COLLECTION ...` | Update collection parameters |
| `DROP COLLECTION name` | Delete a collection |
| `INSERT INTO collection ...` | Insert points with vectors and payload |
| `SELECT * FROM collection WHERE id = ...` | Get a point by ID |
| `SCROLL collection LIMIT n` | Scroll through points |
| `DELETE FROM collection WHERE ...` | Delete by ID, field, or filter |
| `UPDATE collection SET VECTOR ... WHERE id = ...` | Update vectors |
| `UPDATE collection SET PAYLOAD ... WHERE ...` | Set payload on matching points |
| `CREATE INDEX ON collection FOR field TYPE ...` | Create payload index |
| `QUERY 'text' FROM collection LIMIT n` | Vector search (dense, sparse, hybrid, recommend, discover, context, order by, sample, relevance feedback) |

## Filter Expressions

QQL WHERE clauses support complex filtering that maps directly to Qdrant's filter structure:

- **Comparisons**: `=`, `!=`, `>`, `>=`, `<`, `<=` on string, int, float, bool fields
- **Set membership**: `IN (...)` and `NOT IN (...)` on any typed list
- **Null/empty**: `IS NULL`, `IS NOT NULL`, `IS EMPTY`, `IS NOT EMPTY`
- **Text matching**: `MATCH_TEXT`, `MATCH_ANY`, `MATCH_PHRASE`
- **Geo**: `GEO_BOUNDING_BOX`, `GEO_RADIUS`
- **Vector**: `HAS_VECTOR name`
- **Nested**: `NESTED(key, condition)` for nested payload fields
- **Values count**: `VALUES_COUNT(key) > n`
- **Logic**: `AND`, `OR`, `NOT` (full precedence)

All filters compile to `FilterExpr` nodes in the AST. Runtime converts these to Qdrant's REST/gRPC filter shapes.

## Formula Expressions

QQL supports scoring formulas for custom ranking:

- Arithmetic: `+`, `-`, `*`, `/` (with default values), `^`
- Functions: `abs(x)`, `pow(x, y)`
- Variables: payload field references and `score`
- Geo: `geo_distance(lat, lon, field)`
- DateTime: `datetime(expr)`, `datetime_key(field)`
- Decay: `exp_decay`, `gauss_decay`, `linear_decay`
- CASE/WHEN: conditional expressions
- Match conditions: `match_condition(filter_expr, value)`

## Value Types

```rust
pub enum Value<'a> {
    Str(Cow<'a, str>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    List(Vec<Value<'a>>),
    Map(BTreeMap<Cow<'a, str>, Value<'a>>),
}
```

## Transform / Filter Injection

The `inject_filter` function recursively adds a filter condition to all query nodes — main statement and nested CTE prefetches. Designed for tenant isolation and row-level security.

```rust
use qql_core::ast::{Stmt, Value, inject_filter};
use qql_core::parser::Parser;

let mut stmt = Parser::parse("QUERY 'search' FROM docs LIMIT 10").unwrap();
inject_filter(&mut stmt, "org_id", "=", &Value::Str("acme-corp"));
```

Also available for `SCROLL`, `DELETE`, and `UPDATE ... SET PAYLOAD` statements.

## Parser API

```rust
use qql_core::parser::Parser;
use qql_core::ast::Stmt;

// Parse a single statement
let stmt = Parser::parse("SHOW COLLECTIONS")?;

// Parse multiple statements (semicolon-separated)
let stmts = Parser::parse_multi("INSERT INTO docs ...; QUERY 'text' FROM docs ...")?;

// Parse as QueryStmt (fails if not a QUERY)
let query = Parser::parse_query(&full_stmt)?;

// Validate only (no AST returned)
let valid = Parser::is_valid("QUERY 'test' FROM docs LIMIT 10");

// Tokenize without full parsing
let tokens = Parser::tokenize("SELECT * FROM docs WHERE id = 1")?;
```

## Usage

```toml
[dependencies]
qql-core = "0.1"
```

```rust
use qql_core::parser::Parser;
use qql_core::ast::Stmt;

let stmt = Parser::parse("QUERY 'hello' FROM docs LIMIT 5").unwrap();
match stmt {
    Stmt::Query(q) => println!("querying {} with {:?}", q.collection.unwrap(), q.query_text),
    _ => println!("other statement"),
}
```
