# QQL SDK Examples

The examples show the primary Python and Rust flows, plus experimental Node.js
and WASM bindings, in progressively more powerful parser SDK patterns.

## Levels

| # | Level | APIs shown | What it demonstrates |
|---|-------|------------|---------------------|
| 02 | Medium | `inject_filter` | Programmatic WHERE injection — QQL's superpower |
| 03 | Expert | Gateway pattern | Multi-tenant query rewriting with auth policies |

## Running

### Python
```bash
pip install pyqql
# Or: PYTHONPATH=target/release python3 examples/python/01_basic_parse.py
for f in examples/python/*.py; do python3 "$f"; done
```

### Node.js
```bash
npm install nqql
# Or: cp target/release/libnqql.so target/release/nqql.node
for f in examples/nodejs/*.mjs; do node "$f"; done
```

### Go
Use the standalone [qql-go](https://github.com/srimon12/qql-go) library for Go bindings.

### Rust
```bash
for f in examples/rust/*/Cargo.toml; do
  cargo run --manifest-path "$f"
done
```

### WASM (Browser)
```bash
cd crates/qql-wasm && wasm-pack build --target web
# Then serve examples/wasm/ and open in browser
```
