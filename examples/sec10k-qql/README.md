# SEC 10-K Multitenant RAG (pyqql)

Flagship demo: four companies (Honeywell, GE, 3M, RTX), real SEC EDGAR 10-K filings,
hybrid dense+sparse retrieval, and **host-enforced tenant isolation**.

```
Layer 1  SHARD 'honeywell'          physical routing
Layer 2  WHERE tenant_id = …        logical filter
Layer 3  inject_filter + SHARD / stmt.shard_key
```

## What it shows

| Script | Purpose |
|--------|---------|
| `ingest.py` | DROP/CREATE hybrid collection, `is_tenant` index, `CREATE SHARD KEY`, chunk+embed 10-Ks |
| `query.py` | 15 QQL modes: hybrid, USING HYBRID, CTE fusion, MMR, formula, ACORN, COUNT exact, isolation proof |
| `agent.py` | LLM tool-use picks strategy; host always injects filter + shard |

## Requirements

- Qdrant at `QDRANT_URL` (default `http://localhost:6333`)
- OpenAI-compatible embeddings at `EMBED_URL` (default LM Studio `http://127.0.0.1:1234`)
- `pyqql` 0.1.4+ (`pip install -e crates/pyqql` or the workspace wheel)
- Network access to `sec.gov` for filing download

```bash
cd examples/sec10k-qql
pip install -e ../../crates/pyqql
pip install html2text requests   # or: uv sync

python ingest.py
python query.py
python agent.py "What are Honeywell's cybersecurity risks?"
```

## Isolation pattern (copy this)

```python
stmt = pyqql.parse("QUERY TEXT 'risks' FROM sec10k USING HYBRID LIMIT 5")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "honeywell")
stmt.shard_key = "honeywell"  # or write SHARD 'honeywell' in the QQL
client.execute(stmt)
```

Preferred when authoring templates:

```sql
QUERY TEXT 'risks' FROM sec10k USING HYBRID
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  LIMIT 5;
```

No `$tenant` bind params — Qdrant has none. Literals (`SHARD 'honeywell'`) and
host inject lower to the same wire form.

## Schema sketch

```sql
CREATE COLLECTION sec10k
  HYBRID (dense VECTOR(384, COSINE), sparse SPARSE)
  WITH PARAMS (
    shard_number = 8,
    sharding_method = 'custom',
    shard_keys = ['honeywell', 'ge', '3m', 'rtx']
  );

CREATE INDEX ON COLLECTION sec10k FOR tenant_id
  TYPE keyword WITH (is_tenant = true);

CREATE SHARD KEY 'honeywell' ON COLLECTION sec10k WITH (shards_number = 2);
```
