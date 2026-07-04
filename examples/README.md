# QQL SDK Examples

Each language directory contains 3 examples — **basic, medium, expert** — that
showcase the QQL parser SDK in progressively more powerful patterns.

## Levels

| Level | What it shows | Why it matters |
|-------|---------------|----------------|
| **01_basic** | `parse`, `tokenize`, `is_valid` | QQL is an inspectable, programmable language |
| **02_medium** | `inject_filter` with string/list/numeric values | Programmatic WHERE injection — QQL's superpower |
| **03_expert** | Multi-tenant query gateway with auth policies | Production pattern: rewrite queries before Qdrant |

## Core Pattern

All examples follow the same code pattern in each language:

```python
# Parse
ast = parse("QUERY 'hello' FROM docs LIMIT 5")

# Validate
if is_valid(query):
    # Transform
    safe = inject_filter(query, "tenant_id", "=", '{"str": "acme"}')
```

## Running

### Python
```bash
pip install pyqql
python examples/python/01_basic_parse.py
python examples/python/02_medium_inject.py
python examples/python/03_expert_gateway.py
```

### Node.js
```bash
npm install nqql
node examples/nodejs/01_basic_parse.mjs
node examples/nodejs/02_medium_inject.mjs
node examples/nodejs/03_expert_gateway.mjs
```

### Go
```bash
cd examples/go/01_basic_parse && go run main.go
cd examples/go/02_medium_inject && go run main.go
cd examples/go/03_expert_gateway && go run main.go
```

### Rust
```bash
cargo run --manifest-path examples/rust/01_basic_parse/Cargo.toml
cargo run --manifest-path examples/rust/02_medium_inject/Cargo.toml
cargo run --manifest-path examples/rust/03_expert_gateway/Cargo.toml
```

### WASM (Browser)
```bash
# Build first, then serve
cd crates/qql-wasm && wasm-pack build --target web
# Open examples/wasm/*.html in browser
```

## What Makes QQL Different

Most query languages are opaque strings — you send them and hope they work.
QQL gives you **programmatic access** to the query structure:

- **Parse** → understand what a query does
- **Tokenize** → inspect query structure
- **Inject** → add WHERE clauses without string concatenation
- **Validate** → catch errors before hitting production

No string building. No regex. Just clean API calls.
