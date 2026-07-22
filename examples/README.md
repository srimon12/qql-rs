# QQL Examples

Working example applications demonstrating QQL across all language bindings.

| Directory | Language | Description |
|-----------|----------|-------------|
| `python/` | Python (pyqql) | inject_filter + Client + HttpEmbedder |
| `rust/` | Rust (qql-core) | parse + inject_filter + route lower |
| `nodejs/` | Node.js (nqql) | Client + injectFilter + HttpEmbedder |
| `wasm/` | WASM (qql-wasm) | parse + compile + Client (browser fetch) |
| `edge-demo/` | Python (CLI) | Local qdrant-edge HNSW + hybrid search |
| `medical-showcase/` | Python (CLI) | Full retrieval showcase: 12 records, all QQL features |

## Run

All language examples require their respective SDK installed. The CLI-based examples
(`edge-demo/`, `medical-showcase/`) use the `qql` binary:

```bash
# Build the CLI
cargo build --release -p qql-cli --no-default-features --features rest

# Run the medical showcase
QQL_BIN=./target/release/qql uv run examples/medical-showcase/main.py

# Run with execution against Qdrant
QQL_BIN=./target/release/qql uv run examples/medical-showcase/main.py --execute
```

### Python
```bash
cd crates/pyqql && pip install -e .
cd ../../examples/python
python basic_to_medium.py
```

### Rust
```bash
cd examples/rust/basic_to_medium
cargo run
```

### Node.js
```bash
cd crates/nqql && npm install && npm run build
cd ../../examples/nodejs
node basic_to_medium.mjs
```

### WASM
```bash
cd crates/qql-wasm && wasm-pack build --target web
# Then serve examples/wasm/ with any HTTP server
```
