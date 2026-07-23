# 🏠 Berlin Airbnb QQL Geo Showcase Demo

This example demonstrates **QQL's Geo Search Capabilities** (`GEO_RADIUS`, `GEO_BBOX`, `GEO_POLYGON`) combined with multi-tenant custom neighborhood sharding (`SHARD 'mitte'`, `SHARD 'kreuzberg'`) and hybrid text + metadata filtering over real Berlin Airbnb listings data.

---

## 📊 Dataset Highlights

- **Source**: Inside Airbnb Berlin Dataset (`listings.csv.gz`)
- **Location**: Berlin, Germany (Home of Qdrant!)
- **Listings**: ~12,700 real apartments with coordinates (`lat`, `lon`), prices, ratings, and room types.
- **Geo Payload Index**: `location` TYPE `geo`

---

## 🚀 Quickstart Ingestion

1. Ensure your local Qdrant instance is running on `http://localhost:6333`.
2. Run the ingestion pipeline:

```bash
python3 ingest.py
```

This script will automatically:
1. Create the `berlin_airbnb` collection with custom neighborhood sharding.
2. Build payload indexes for `location` (`geo`), `neighbourhood` (`keyword`), `price` (`float`), `room_type` (`keyword`), and `rating` (`float`).
3. Embed listing titles and descriptions using `all-MiniLM-L6-v2`.
4. Ingest listing points into their respective neighborhood shards (`mitte`, `pankow`, `kreuzberg`, etc.).

---

## 🔍 Sample QQL Geo Queries

### 1. GEO_RADIUS (Brandenburg Gate / Central Berlin)
Search for cozy apartments within 1,500 meters of Brandenburg Gate (`lat=52.5163`, `lon=13.3777`) under €100/night:

```sql
QUERY TEXT 'cozy studio near historic landmarks'
FROM berlin_airbnb
WHERE location GEO_RADIUS {center: {lat: 52.5163, lon: 13.3777}, radius: 1500.0}
  AND price <= 100.0
LIMIT 5;
```

---

### 2. GEO_BBOX (Manhattan / Mitte City Center)
Search for spacious lofts inside the Mitte bounding box (`top_left={lat: 52.535, lon: 13.360}`, `bottom_right={lat: 52.505, lon: 13.420}`):

```sql
QUERY HYBRID TEXT 'spacious loft with balcony and fast wifi'
FROM berlin_airbnb
WHERE location GEO_BBOX {top_left: {lat: 52.535, lon: 13.360}, bottom_right: {lat: 52.505, lon: 13.420}}
  AND room_type = 'Entire home/apt'
LIMIT 5;
```

---

### 3. GEO_POLYGON (Kreuzberg Nightlife District Boundary)
Search for top-rated artistic flats inside a custom neighborhood polygon boundary:

```sql
QUERY TEXT 'artistic flat nightlife and coffee shops'
FROM berlin_airbnb
WHERE location GEO_POLYGON {exterior: [{lat: 52.500, lon: 13.370}, {lat: 52.515, lon: 13.430}, {lat: 52.485, lon: 13.450}, {lat: 52.470, lon: 13.390}, {lat: 52.500, lon: 13.370}]}
  AND rating >= 4.7
LIMIT 5;
```

---

### 4. Custom Neighborhood Shard Isolation
Search specifically within the `mitte` custom physical shard:

```sql
QUERY TEXT 'quiet apartment courtyard'
FROM berlin_airbnb
SHARD 'mitte'
WHERE rating >= 4.5
LIMIT 5;
```
