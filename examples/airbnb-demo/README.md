# Berlin Airbnb — QQL Geo Showcase

Real [Inside Airbnb](http://insideairbnb.com/) Berlin listings with **geo filters**,
**hybrid search**, and **turbo quantization + rescore**.

No custom district shards — filter with `WHERE district = 'Mitte'` instead.

## Stack

| Piece | Choice |
|-------|--------|
| Dense | Ollama `all-minilm:l6-v2` (384-d) |
| Sparse | Local hashed BM25-style vectors |
| Quant | `turbo` bits=2 (or 4 via `QUANT_BITS`) `always_ram` |
| Query | `PARAMS (quantization = {rescore: true, oversampling: 2.0})` |

## Quickstart

```bash
# Qdrant :6333 + Ollama with all-minilm:l6-v2
cd examples/airbnb-demo
pip install -e ../../crates/pyqql

python ingest.py                 # default: 1500 listings, turbo/2
QUANT_BITS=4 MAX_LISTINGS=3000 python ingest.py

python query.py                  # parse-check
python query.py --execute        # live geo + hybrid + rescore
```

## Sample QQL

```sql
-- Geo radius + quant rescore
QUERY TEXT 'cozy studio near historic landmarks'
FROM berlin_airbnb
USING dense
WHERE location GEO_RADIUS {
    center: {lat: 52.5163, lon: 13.3777}, radius: 1500.0
  }
  AND price <= 100.0
PARAMS (hnsw_ef = 128, quantization = {ignore: false, rescore: true, oversampling: 2.0})
LIMIT 5;

-- District filter (not a shard key)
QUERY TEXT 'quiet courtyard'
FROM berlin_airbnb
USING dense
WHERE district = 'Mitte' AND rating >= 4.5
PARAMS (quantization = {rescore: true, oversampling: 2.0})
LIMIT 5;
```

## Layout

| File | Role |
|------|------|
| `config.py` | Ollama URL, turbo bits, landmarks |
| `ingest.py` | Schema + turbo quant + hybrid UPSERT |
| `query.py` | 12 geo/hybrid/formula demos with rescore |
| `listings.csv.gz` | Inside Airbnb Berlin dump |
