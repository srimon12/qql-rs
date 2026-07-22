# QQL Installation

## Rust CLI (`qql`)

### From Source

```bash
git clone https://github.com/srimon12/qql-rs.git
cd qql-rs
cargo build --release -p qql-cli --no-default-features --features rest

# Optional: install globally
cargo install --path crates/qql-cli --no-default-features --features rest
```

The binary will be at `target/release/qql`.

### Features

| Feature | Description |
|---------|-------------|
| `rest` | HTTP REST client (reqwest) — enabled by default |
| `grpc` | gRPC client (tonic) — for Qdrant port 6334 |
| `edge` | In-process execution via qdrant-edge (no server needed) |

Build with gRPC: `cargo build --release -p qql-cli --no-default-features --features rest,grpc`

Build with edge (local HNSW + fastembed): `cargo build --release -p qql-cli`

### Verify Installation

```bash
./target/release/qql version
```

## Rust Library (`qql`)

Add to `Cargo.toml`:

```toml
[dependencies]
qql = { path = "crates/qql-runtime" }
qql-core = { path = "crates/qql-core" }
qql-plan = { path = "crates/qql-plan" }
```

### Basic Usage

```rust
use qql::executor::Executor;
use qql::rest::RestQdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    let exec = Executor::new(ops, None);
    let res = exec.execute("SHOW COLLECTIONS").await?;
    println!("{}", serde_json::to_string_pretty(&res)?);
    Ok(())
}
```

## Python SDK (`pyqql`)

```bash
pip install maturin
cd crates/pyqql
maturin develop --release
```

```python
import pyqql

client = pyqql.Client("http://localhost:6333")
result = client.execute("QUERY 'search' FROM docs USING dense LIMIT 5")
print(result)
```

## Node.js SDK (`nqql`)

```bash
cd crates/nqql
npm install
npm run build
```

```javascript
const nqql = require('nqql');
const client = new nqql.Client("http://localhost:6333", null);
const result = client.execute("QUERY 'search' FROM docs USING dense LIMIT 5");
console.log(result);
```

## WASM SDK (`qql-wasm`)

```bash
cd crates/qql-wasm
wasm-pack build --target web
```

```javascript
import init, { Client, parse } from './pkg/qql_wasm.js';

await init();
const stmt = parse("QUERY 'hello' FROM docs LIMIT 5");
console.log(JSON.parse(stmt));
```
