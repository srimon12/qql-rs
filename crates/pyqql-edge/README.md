# pyqql-edge

Local QQL for Python: **qdrant-edge + FastEmbed**, zero remote Qdrant.

## Proposition

Same QQL language as `pyqql`, but storage and (optionally) embeddings run
**in-process** (qdrant-edge **0.8**). Ideal for demos, CI, air-gapped tools.
Cluster-only features (custom `SHARD`, `GROUP BY … LOOKUP FROM`, **`SHOW
QUOTAS` / `SET QUOTA`**, …) fail with stable `QQL-EDGE-UNSUPPORTED-*` codes.
Sparse `PARAMS (idf = …)`, ACORN search params, `GROUP BY`, and
HNSW/optimizer `ALTER COLLECTION` are supported offline.

## Install

```bash
pip install pyqql-edge
```

Python 3.10+. Wheels: Linux x64, macOS arm64, Windows x64 (not macOS Intel — ONNX).

## Quick start

```python
import pyqql_edge

client = pyqql_edge.local_executor(
    "./qdrant_data",
    on_disk_payload=False,
    model="bge-small-en-v1.5",
)

client.execute("CREATE COLLECTION docs HYBRID")
client.execute(
    'UPSERT INTO docs VALUES {id: 1, text: "hello from edge"}'
)
report = client.execute("QUERY TEXT 'hello' FROM docs USING dense LIMIT 10")
print(report)

stmt = pyqql_edge.parse("QUERY TEXT 'hello' FROM docs USING dense LIMIT 10")[0]
pyqql_edge.inject_filter(stmt, "org_id", "=", "acme")
# Edge has no custom SHARD routing — use remote Qdrant for SHARD / CREATE SHARD KEY

client.close()
```

### WAL footprint

qdrant-edge pre-allocates each write-ahead-log segment (default 32 MiB), which
dominates the on-disk footprint of small embedded shards. Pass whole MiB to
`wal_segment_mb` to shrink it; the resolved capacity persists in the shard's
`edge_config.json`:

```python
client = pyqql_edge.local_executor("./qdrant_data", wal_segment_mb=8)
```

`None` (the default) keeps the 32 MiB engine default. Zero, negative,
fractional, or overflowing values fail closed with `QQL-VALIDATION-CONFIG`.
qdrant-edge 0.8's own Python binding does not expose this knob.

## API

| Export | Role |
|--------|------|
| `local_executor(data_dir, …)` | FastEmbed + edge storage |
| `http_executor(data_dir, url, …)` | Edge storage + remote HTTP embedder |
| `list_embedding_models()` | Dense ONNX catalog |
| `parse` / `parse_json` / `is_valid` / `tokenize` | Frontend |
| `inject_filter` | Isolation |
| `Stmt.shard_key` | Property exists for AST parity; **edge rejects SHARD at execute** |
| `bind(query, params)` | Substitute `:name` (dict) or `?` (list) |
| `compile_query` / `explain` / `execute` | Plan / run (`params=` same as `bind`) |

## Edge gotchas

| Topic | Reality |
|-------|---------|
| Point IDs | Integers or UUIDs only |
| HYBRID queries | Specify `USING dense` / `USING sparse` / hybrid forms |
| `SHARD`, quotas | Unsupported offline — use remote Qdrant (`QQL-EDGE-UNSUPPORTED-*`) |
| Sparse `idf` | Supported (qdrant-edge 0.8) |
| Models | Locked at executor construction |
| Lifetime | Call `close()` before deleting `data_dir` |

## Docs

- [qql-edge](https://github.com/srimon12/qql-rs/blob/main/crates/qql-edge/README.md) · [Gaps](https://github.com/srimon12/qql-rs/blob/main/skills/qql-skill/references/qql-gaps.md) · [Syntax](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
