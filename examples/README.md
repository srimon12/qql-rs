# QQL Examples

Working demos of **QQL 1.2** across language bindings and end-to-end apps.

Two flagship stories:

| Demo | Story |
|------|--------|
| **[sec10k-qql/](sec10k-qql/)** | Multi-tenant RAG over real SEC 10-K filings — `inject_filter` + `inject_shard_key` |
| **[airbnb-demo/](airbnb-demo/)** | Berlin geo search — `GEO_RADIUS` / `BBOX` / `POLYGON` + district shards |

## Catalog

| Directory | Stack | What it teaches |
|-----------|-------|-----------------|
| [`python/`](python/) | pyqql | Offline parse → explain → compile → inject filter/shard; multi-tenant gateway |
| [`nodejs/`](nodejs/) | nqql | Same gateway pattern in Node (N-API) |
| [`rust/`](rust/) | qql-core | `ComparisonOp` inject API + fail-closed shard inject |
| [`wasm/`](wasm/) | qql-wasm | Browser `Stmt` / `analyze` / hybrid compile |
| [`sec10k-qql/`](sec10k-qql/) | pyqql + LM Studio | Full multitenant hybrid RAG + agent tool-use |
| [`airbnb-demo/`](airbnb-demo/) | pyqql | Geo filters, hybrid, custom district sharding |
| [`medical-showcase/`](medical-showcase/) | qql CLI | 12-record E2E: hybrid, recommend, CTE, mutations, COUNT exact |
| [`edge-demo/`](edge-demo/) | qql CLI `--edge` | Zero-network qdrant-edge + fastembed |
| [`medical-retrieval-ops/`](medical-retrieval-ops/) | qql CLI + ChatMED | Larger medical corpus + benchmark harness |

## Language bindings (offline-first)

These scripts validate grammar and host UX without Qdrant:

```bash
# Python
cd crates/pyqql && pip install -e .
python ../../examples/python/basic_to_medium.py
python ../../examples/python/medium_to_expert.py

# Rust
cd examples/rust/basic_to_medium && cargo run
cd ../medium_to_expert && cargo run

# Node.js
cd crates/nqql && npm install && npm run build
node ../../examples/nodejs/basic_to_medium.mjs
node ../../examples/nodejs/medium_to_expert.mjs

# WASM (needs demo/pkg or wasm-pack build)
# Serve examples/ + demo/ and open the JS modules in a browser, or run under node with wasm
```

### Multi-tenant pattern (all SDKs)

```python
stmt = pyqql.parse("QUERY TEXT 'risks' FROM docs USING HYBRID LIMIT 5")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "acme")
pyqql.inject_shard_key(stmt, "acme")
client.execute(stmt)
```

```rust
use qql_core::ast::{inject_filter, inject_shard_key, ComparisonOp, Value};
// ...
inject_filter(&mut stmt, "tenant_id", ComparisonOp::Eq, Value::Str("acme".into()))?;
inject_shard_key(&mut stmt, "acme")?;
```

```js
const [stmt] = parse("QUERY TEXT 'risks' FROM docs USING HYBRID LIMIT 5");
stmt.injectFilter("tenant_id", "=", "acme");
stmt.injectShardKey("acme");
```

## Flagship demos

### SEC 10-K multitenancy

```bash
# Needs Qdrant + OpenAI-compatible embeddings (LM Studio / Ollama)
cd examples/sec10k-qql
pip install html2text requests
python ingest.py
python query.py
python agent.py "What are Honeywell's cybersecurity risks?"
```

### Berlin Airbnb geo

```bash
cd examples/airbnb-demo
python ingest.py              # hash vectors offline; or EMBED_URL=… for real dense
python query.py               # parse-check all geo/hybrid queries
python query.py --execute     # live search
```

## CLI demos

```bash
# Build CLI (REST)
cargo build --release -p qql-cli --no-default-features --features rest

# Medical showcase (print-only or --execute)
QQL_BIN=./target/release/qql python examples/medical-showcase/main.py
QQL_BIN=./target/release/qql python examples/medical-showcase/main.py --execute --keep

# Edge (default CLI features include edge + fastembed)
cargo build --release -p qql-cli
QQL_BIN=./target/release/qql python examples/edge-demo/main.py
QQL_BIN=./target/release/qql python examples/edge-demo/main.py --dry-run
```

## QQL 1.2 features covered

Examples across this folder exercise:

- `USING HYBRID` / `QUERY HYBRID` … `FUSION RRF|DBSF`
- `inject_filter` + `inject_shard_key` (fail-closed on DDL)
- `CREATE SHARD KEY` / custom `sharding_method`
- `GEO_RADIUS` / `GEO_BBOX` / `GEO_POLYGON` + formula `GEO_DISTANCE`
- `PARAMS (acorn, max_selectivity, timeout, consistency, hnsw_ef)`
- `COUNT … WITH (exact = true)`
- `SCROLL … WITH VECTOR false`
- `DELETE PAYLOAD key FROM …`
- CTE `PREFETCH` + `FORMULA` + `MMR` + `GROUP BY` + `ORDER BY`
- Recommend / Context / Discover (medical showcase)

## Version note

Target SDK / engine: **0.1.4+**. Older release binaries (e.g. 0.1.1 CLI) reject some 1.2 syntax — rebuild from this workspace:

```bash
cargo build --release -p qql-cli
```
