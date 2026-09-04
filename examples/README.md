# QQL Examples

Working demos of **QQL 1.5** (Qdrant ≥ 1.19.0) across language bindings and end-to-end apps.

Two flagship stories:

| Demo | Story |
|------|--------|
| **[sec10k-qql/](sec10k-qql/)** | Multi-tenant RAG over real SEC 10-K filings — `inject_filter` + `SHARD` / `stmt.shard_key` |
| **[airbnb-demo/](airbnb-demo/)** | Berlin geo search — `GEO_RADIUS` / `BBOX` / `POLYGON` + district shards |

## QQL 1.5 / Qdrant 1.19 snippets

These parse offline; execute against Qdrant 1.19+ (quotas are REST-only):

```sql
-- Memory tiers + turbo4 dense storage
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE)
    WITH VECTOR (memory = 'cached', datatype = 'turbo4')
    WITH HNSW (memory = 'cold')
) WITH PARAMS (payload_memory = 'cold');

CREATE INDEX ON COLLECTION docs FOR title TYPE keyword WITH (prefix = true);

-- Prefix match + deterministic slice sampling
QUERY TEXT 'compliance' FROM docs
  WHERE title MATCH PREFIX 'Comp' AND SLICE (4, 0)
  LIMIT 20;

-- Per-query sparse IDF corpus (tenant stats)
QUERY TEXT 'risks' FROM docs USING sparse
  WHERE tenant_id = 'acme'
  SHARD 'acme'
  PARAMS (idf = WHERE tenant_id = 'acme')
  LIMIT 10;

-- In-database faceting / categorical value aggregation
FACET category FROM docs WHERE rating >= 4.0 LIMIT 10;

-- Compact vector literal & default payload inclusion (WITH PAYLOAD true is default)
QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5;

-- Cluster resource quotas (REST only; not edge/gRPC)
SHOW QUOTAS;
SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;
```

Route affinity (`X-Qdrant-Route-Affinity`) is a **client transport** option on
every remote SDK — `pyqql.Client(route_affinity=…)`, `nqql`
`new Client({ routeAffinity })`, WASM `client.setRouteAffinity(key)`, and Rust
`RestQdrant` / `GrpcQdrant::with_route_affinity` — not QQL syntax. The `--live`
examples below construct their client with it.

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

# WASM (checked-in Node build of qql-wasm)
node examples/wasm/basic_to_medium.js
node examples/wasm/medium_to_expert.js
```

### Multi-tenant pattern (all SDKs)

```python
# Prefer SHARD in QQL when the tenant is known:
#   QUERY TEXT 'risks' FROM docs USING HYBRID SHARD 'acme' LIMIT 5
stmt = pyqql.parse("QUERY TEXT 'risks' FROM docs USING HYBRID LIMIT 5")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "acme")  # isolation (always)
stmt.shard_key = "acme"  # optional host routing; same field as SHARD
client.execute(stmt)
```

```rust
use qql_core::ast::{inject_filter, ComparisonOp, Value};
// ...
inject_filter(&mut stmt, "tenant_id", ComparisonOp::Eq, Value::Str("acme".into()))?;
stmt.set_shard_key(Some("acme".into())); // optional; prefer SHARD in QQL
```

```js
const [stmt] = parse("QUERY TEXT 'risks' FROM docs USING HYBRID LIMIT 5");
stmt.injectFilter("tenant_id", "=", "acme");
stmt.shardKey = "acme"; // optional; prefer SHARD 'acme' in QQL
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

# Edge (requires edge feature)
cargo build --release -p qql-cli --features edge
QQL_BIN=./target/release/qql python examples/edge-demo/main.py
QQL_BIN=./target/release/qql python examples/edge-demo/main.py --dry-run
```

## QQL 1.5 / Qdrant 1.19 features covered

Examples across this folder exercise:

- `USING HYBRID` / `QUERY HYBRID` … `FUSION RRF|DBSF`
- `inject_filter` + `SHARD` / `stmt.shard_key` (fail-closed on DDL)
- `CREATE SHARD KEY` / custom `sharding_method`
- `GEO_RADIUS` / `GEO_BBOX` / `GEO_POLYGON` + formula `GEO_DISTANCE`
- `PARAMS (acorn, max_selectivity, timeout, consistency, hnsw_ef, idf)`
- `WHERE title MATCH PREFIX '…'` + keyword index `prefix = true` (Qdrant 1.19+)
- `WHERE SLICE (total, index)` deterministic sampling (Qdrant 1.19+)
- Memory placement (`memory` / `payload_memory`) + dense `datatype = 'turbo4'`
- `SHOW QUOTAS` / `SET QUOTA (…)` (REST only)
- `FACET <field> FROM …` in-database categorical aggregation
- `QUERY […]` compact vector array literals (optional `VECTOR` keyword)
- Automatic point payload inclusion (`WITH PAYLOAD true` is default)
- Formula ISO 8601 decay targets (`TARGET = "2024-01-01T00:00:00Z"` with inferred `datetime_key`)
- `COUNT … WITH (exact = true)`
- `SCROLL … WITH VECTOR false`
- `DELETE PAYLOAD key FROM …`
- `CTE` `PREFETCH` + `FORMULA` + `MMR` + `GROUP BY` + `ORDER BY`
- Recommend / Context / Discover (medical showcase)
- Route affinity (`pyqql` / `nqql` / wasm / Rust client options) in the `--live` paths

## Version note

Target SDK / engine: **0.3.0+**. Older release binaries reject some 1.3 syntax — rebuild from this workspace:

```bash
cargo build --release -p qql-cli
```
