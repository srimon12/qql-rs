# QQL SDK Examples

Each language directory contains 4 examples — **basic, medium, expert,
high-perf** — that showcase the QQL parser SDK in progressively more
powerful patterns.

## Levels

| # | Level | APIs shown | What it demonstrates |
|---|-------|------------|---------------------|
| 01 | Basic | `parse`, `tokenize`, `is_valid` | QQL is an inspectable, programmable language |
| 02 | Medium | `inject_filter` | Programmatic WHERE injection — QQL's superpower |
| 03 | Expert | Gateway pattern | Multi-tenant query rewriting with auth policies |
| 04 | High-Perf | `parse_all`, `parse_batch` | Script parsing and batch FFI for throughput |

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
```bash
for d in examples/go/*/; do
  cd "$d" && CGO_LDFLAGS="-L$(pwd)/../../../target/release -l:libgqql.a -lm" go run main.go
done
```

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
