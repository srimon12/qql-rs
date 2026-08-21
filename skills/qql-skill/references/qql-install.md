# QQL Installation

Install the pieces you need. Runtime can talk **REST (6333)** and/or **gRPC (6334)** —
both are first-class; pick at client construction time.

## Backend version notes

| Feature set | Minimum backend |
|-------------|-----------------|
| Core QQL search / hybrid / multivector | Qdrant **1.x** (protocol pin tracks openapi/proto in tree) |
| Quotas, memory placement, `turbo4`, `MATCH PREFIX`, `SLICE`, query `idf` | Qdrant **1.19.0+** |
| Edge quotas | **Unsupported** (`QQL-EDGE-UNSUPPORTED-QUOTA`) |
| Edge IDF search params | qdrant-edge **0.8+** |
| gRPC quotas | **Unsupported** (`QQL-GRPC-QUOTA`) — use REST for `SHOW QUOTAS` / `SET QUOTA` |

For local development against 1.19 features, run a Qdrant **1.19** (or newer) server
on `6333`/`6334`. Older servers will reject new config keys / filter conditions.

## Rust CLI (`qql`)

### From source

```bash
git clone https://github.com/srimon12/qql-rs.git
cd qql-rs

# REST-only CLI (fast build)
cargo build --release -p qql-cli --no-default-features --features rest

# REST + gRPC
cargo build --release -p qql-cli --no-default-features --features rest,grpc

# Full CLI including edge + fastembed
cargo build --release -p qql-cli --features edge
```

Binary: `target/release/qql`.

| Feature | Description |
|---------|-------------|
| `rest` | HTTP client → Qdrant REST |
| `grpc` | tonic client → Qdrant gRPC |
| `edge` | In-process qdrant-edge (no server) |

### CLI Commands

```bash
# Execute a query
qql exec "QUERY 'hello' FROM docs USING dense LIMIT 5" --json

# Execute from file
qql execute script.qql --stop-on-error

# Explain (no Qdrant needed)
qql explain "QUERY 'hello' FROM docs USING dense LIMIT 5"

# Interactive REPL
qql connect

# Dump collection to QQL
qql dump my_collection output.qql

# Health check
qql doctor
```

### Environment

| Variable | Role |
|----------|------|
| `QDRANT_URL` | REST base (default `http://localhost:6333`) |
| `QDRANT_API_KEY` | Qdrant API key |
| `EMBED_URL` | OpenAI-compatible embeddings endpoint (e.g. Ollama `…/v1/embeddings`) |
| `EMBED_MODEL` | e.g. `all-minilm:l6-v2` |
| `EMBED_DIM` | e.g. `384` |

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
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exec = Executor::rest("http://localhost:6333", None)?;
    let res = exec.execute("SHOW COLLECTIONS", OnError::Stop).await?;
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
const nqql = require('@veristamp/nqql');
const client = new nqql.Client({ url: "http://localhost:6333" });
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 5");
console.log(result);
```

## WASM SDK (`qql-wasm`)

```bash
cd crates/qql-wasm
wasm-pack build --target web
```

```javascript
import init, { parse, Client } from './pkg/qql_wasm.js';

await init();
const result = await (new Client("http://localhost:6333")).execute("QUERY 'hello' FROM docs LIMIT 5");
console.log(result);
```
