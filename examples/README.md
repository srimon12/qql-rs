# QQL SDK Examples

Clean, production-ready SDK examples across Python, Node.js, WebAssembly, and Rust.

## Examples Structure

Each language directory contains two unnumbered example scripts:

1. **`basic_to_medium`**: Connection initialization, basic execution, plan explanation, and AST filter injection.
2. **`medium_to_expert`**: First-class HTTP embedding provider integration (`HttpEmbedder`), complex CTE prefetch DAGs with RRF fusion, and multi-tenant security gateways.

| Language | Basic to Medium | Medium to Expert |
|---|---|---|
| **Python** | `examples/python/basic_to_medium.py` | `examples/python/medium_to_expert.py` |
| **Node.js** | `examples/nodejs/basic_to_medium.mjs` | `examples/nodejs/medium_to_expert.mjs` |
| **WASM** | `examples/wasm/basic_to_medium.js` | `examples/wasm/medium_to_expert.js` |
| **Rust** | `examples/rust/basic_to_medium` | `examples/rust/medium_to_expert` |

## Running Examples

### Python
```bash
python3 examples/python/basic_to_medium.py
python3 examples/python/medium_to_expert.py
```

### Node.js
```bash
node examples/nodejs/basic_to_medium.mjs
node examples/nodejs/medium_to_expert.mjs
```

### Rust
```bash
cargo run --manifest-path examples/rust/basic_to_medium/Cargo.toml
cargo run --manifest-path examples/rust/medium_to_expert/Cargo.toml
```
