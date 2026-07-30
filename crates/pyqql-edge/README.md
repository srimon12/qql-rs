# pyqql-edge

Local QQL for Python: **qdrant-edge + FastEmbed**, zero remote Qdrant.

## Proposition

Same QQL language as `pyqql`, but storage and (optionally) embeddings run
**in-process**. Ideal for demos, CI, air-gapped tools. Cluster-only features
(`GROUP BY`, custom `SHARD`, ACORN, …) fail with stable `QQL-EDGE-UNSUPPORTED-*` codes.

## Install

```bash
pip install pyqql-edge
```

Python 3.8+. Wheels: Linux x64, macOS arm64, Windows x64 (not macOS Intel — ONNX).

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

## API

| Export | Role |
|--------|------|
| `local_executor(data_dir, …)` | FastEmbed + edge storage |
| `http_executor(data_dir, url, …)` | Edge storage + remote HTTP embedder |
| `list_embedding_models()` | Dense ONNX catalog |
| `parse` / `parse_json` / `is_valid` / `tokenize` | Frontend |
| `inject_filter` | Isolation |
| `Stmt.shard_key` | Property exists for AST parity; **edge rejects SHARD at execute** |
| `compile_query` / `explain` / `execute` | Plan / run |

## Edge gotchas

| Topic | Reality |
|-------|---------|
| Point IDs | Integers or UUIDs only |
| HYBRID queries | Specify `USING dense` / `USING sparse` / hybrid forms |
| `GROUP BY`, `SHARD`, ACORN | Unsupported offline — use remote Qdrant |
| Models | Locked at executor construction |
| Lifetime | Call `close()` before deleting `data_dir` |

## Docs

- [qql-edge](../qql-edge/README.md) · [Gaps](../../skills/qql-skill/references/qql-gaps.md) · [Syntax](../../docs/syntax.md)
