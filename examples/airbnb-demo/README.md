# Berlin Airbnb — QQL Geo Showcase

Real [Inside Airbnb](http://insideairbnb.com/) Berlin listings demonstrating
**geo filters**, **hybrid search**, and **district custom sharding**.

Berlin is home to Qdrant — this demo keeps the geo story front and center.

## Features

| Capability | Example |
|------------|---------|
| `GEO_RADIUS` | Listings within 1.5 km of Brandenburg Gate |
| `GEO_BBOX` | Mitte city-center bounding box |
| `GEO_POLYGON` | Kreuzberg nightlife polygon |
| Hybrid | `USING HYBRID DENSE dense SPARSE sparse FUSION RRF` |
| Custom shards | `SHARD 'mitte'`, `SHARD 'kreuzberg'`, … via `inject_shard_key` |
| Formula | `GAUSS_DECAY(GEO_DISTANCE(…))` for distance-aware ranking |
| Indexes | `location` geo, `district` `is_tenant`, price/rating/room_type |

## Dataset

- Source: Inside Airbnb Berlin (`listings.csv.gz` bundled)
- ~12.7k listings with lat/lon, price, ratings, room types
- Demo default: first **2500** rows (`MAX_LISTINGS=0` for all)

## Quickstart

```bash
# Qdrant on localhost:6333
cd examples/airbnb-demo
pip install -e ../../crates/pyqql   # pyqql 0.1.4+

# Offline hash vectors (no embed server)
python ingest.py

# Or real dense embeddings
EMBED_URL=http://localhost:11434 MAX_LISTINGS=1000 python ingest.py

# Parse-check all showcase queries (no Qdrant)
python query.py

# Live search
python query.py --execute
```

## Sample QQL

### GEO_RADIUS (Brandenburg Gate)

```sql
QUERY TEXT 'cozy studio near historic landmarks'
FROM berlin_airbnb
USING dense
WHERE location GEO_RADIUS {
    center: {lat: 52.5163, lon: 13.3777},
    radius: 1500.0
  }
  AND price <= 100.0
LIMIT 5;
```

### GEO_BBOX + hybrid

```sql
QUERY TEXT 'spacious loft with balcony'
FROM berlin_airbnb
USING HYBRID DENSE dense SPARSE sparse FUSION RRF
WHERE location GEO_BBOX {
    top_left: {lat: 52.535, lon: 13.360},
    bottom_right: {lat: 52.505, lon: 13.420}
  }
  AND room_type = 'Entire home/apt'
LIMIT 5;
```

### District shard isolation (host inject)

```python
stmt = pyqql.parse("""
  QUERY TEXT 'quiet courtyard apartment'
  FROM berlin_airbnb USING dense
  WHERE rating >= 4.5 LIMIT 5
""")[0]
pyqql.inject_shard_key(stmt, "mitte")
client.execute(stmt)
```

### Formula geo-decay

```sql
WITH candidates AS (
  QUERY TEXT 'apartment near museums' FROM berlin_airbnb USING dense LIMIT 50
)
QUERY FORMULA (
  score * GAUSS_DECAY(GEO_DISTANCE(52.5163, 13.3777, location), 0.0, 3000.0, 0.5)
) DEFAULTS (location = {lat: 52.5163, lon: 13.3777})
FROM berlin_airbnb
PREFETCH (candidates)
LIMIT 5;
```

## Layout

| File | Role |
|------|------|
| `config.py` | URLs, shard map, landmark coordinates |
| `ingest.py` | Schema + indexes + sharded UPSERT |
| `query.py` | 10 geo/hybrid/formula demos |
| `listings.csv.gz` | Inside Airbnb Berlin dump |
| `neighbourhoods.geojson` | Optional neighbourhood polygons |
