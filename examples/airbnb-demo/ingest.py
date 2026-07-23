"""
Berlin Airbnb Rich QQL Showcase Ingestion Pipeline

Extracts listing text + rich payload metadata:
- Geo Location ({lat, lon}) for GEO_RADIUS, GEO_BBOX, GEO_POLYGON queries
- Price (float) & Rating (float) for WHERE filters & FORMULA score boosting
- Superhost (bool) & Instant Bookable (bool) for CASE WHEN business logic scoring
- Neighbourhood (keyword), Room Type (keyword), Accommodates (integer) for GROUP BY & filters

Generates 384-dimensional dense vectors directly in Python and executes QQL DDL + UPSERT.
"""

import sys
import os
import gzip
import csv
import re
import math
import hashlib

try:
    import pyqql
except ImportError:
    for path in [
        os.environ.get("QQL_LIB", ""),
        os.path.abspath(os.path.join(os.path.dirname(__file__), "../../target/release")),
        os.path.abspath(os.path.join(os.path.dirname(__file__), "../../crates/pyqql")),
    ]:
        if path and path not in sys.path:
            sys.path.insert(0, path)
    import pyqql

import config

def clean_price(val: str) -> float:
    if not val:
        return 0.0
    cleaned = re.sub(r"[^\d.]", "", val)
    try:
        return float(cleaned)
    except ValueError:
        return 0.0

def clean_string(val: str) -> str:
    if not val:
        return ""
    # Strip HTML tags
    s = re.sub(r"<[^>]+>", " ", val)
    # Strip quotes, backslashes, braces, and non-alphanumeric punctuation
    s = re.sub(r"[^\w\s.,!?-]", " ", s)
    return re.sub(r"\s+", " ", s).strip()

def text_to_vector(text: str, dim: int = 384) -> list[float]:
    """Generate a 384-d normalized dense vector directly from text."""
    vec = [0.0] * dim
    words = text.lower().split()
    for word in words:
        h = hashlib.sha256(word.encode()).digest()
        for i in range(dim):
            vec[i] += (h[i % len(h)] - 128) / 128.0
    norm = math.sqrt(sum(x * x for x in vec)) or 1.0
    return [round(x / norm, 5) for x in vec]

def load_listings():
    csv_path = os.path.join(os.path.dirname(__file__), "listings.csv.gz")
    if not os.path.exists(csv_path):
        raise FileNotFoundError(f"Missing listings dataset at {csv_path}")

    listings = []
    with gzip.open(csv_path, "rt", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            try:
                lat = float(row.get("latitude", 0))
                lon = float(row.get("longitude", 0))
                if lat == 0 or lon == 0:
                    continue
                
                lid = int(row.get("id", 0))
                if not lid:
                    continue

                name = clean_string(row.get("name") or "")
                desc = clean_string(row.get("description") or "")
                text = f"{name}. {desc}".strip()
                if not text:
                    text = f"Airbnb listing {lid}"

                neighborhood = clean_string(row.get("neighbourhood_group_cleansed") or row.get("neighbourhood_cleansed") or "Mitte")
                price = clean_price(row.get("price", ""))
                room_type = clean_string(row.get("room_type", "Entire home/apt"))
                accommodates = int(float(row.get("accommodates") or 2))
                rating = float(row.get("review_scores_rating") or 4.5)
                superhost = (row.get("host_is_superhost") or "").lower() == "t"
                instant_bookable = (row.get("instant_bookable") or "").lower() == "t"
                reviews_count = int(float(row.get("number_of_reviews") or 0))

                vec = text_to_vector(text, config.EMBED_DIM)

                listings.append({
                    "id": lid,
                    "text": text[:350],
                    "name": name[:100],
                    "neighborhood": neighborhood,
                    "lat": lat,
                    "lon": lon,
                    "price": price,
                    "room_type": room_type,
                    "accommodates": accommodates,
                    "rating": rating,
                    "superhost": superhost,
                    "instant_bookable": instant_bookable,
                    "reviews_count": reviews_count,
                    "vector": vec,
                })
                if config.MAX_LISTINGS and len(listings) >= config.MAX_LISTINGS:
                    break
            except Exception:
                continue
    return listings

def main():
    print("Loading Berlin Airbnb dataset from listings.csv.gz...")
    listings = load_listings()
    print(f"Loaded {len(listings)} listings across Berlin neighborhoods.")

    client = pyqql.Client(config.QDRANT_URL)

    # ── Setup Collection & Indexes ──
    print(f"Setting up collection '{config.COLLECTION}'...")
    try:
        client.execute(f"DROP COLLECTION {config.COLLECTION};")
    except Exception:
        pass

    # Create Collection with Dense Vector
    client.execute(f"""
        CREATE COLLECTION {config.COLLECTION}
        (dense VECTOR({config.EMBED_DIM}, COSINE));
    """)

    # Create Payload Indexes for Geo, Keyword, Numeric, and Boolean filtering
    index_queries = [
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR location TYPE geo;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR neighbourhood TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR price TYPE float;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR rating TYPE float;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR room_type TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR accommodates TYPE integer;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR superhost TYPE bool;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR instant_bookable TYPE bool;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR reviews_count TYPE integer;",
    ]
    for q in index_queries:
        for stmt in pyqql.parse_all(q):
            try:
                client.execute(stmt)
            except Exception as e:
                print(f"  Index notice: {e}")

    # ── Ingest Listings ──
    print(f"Ingesting {len(listings)} listings into Qdrant via pyqql...")
    batch_size = 50
    total_ingested = 0

    for i in range(0, len(listings), batch_size):
        batch = listings[i:i + batch_size]
        vals = []
        for item in batch:
            superhost_str = "true" if item["superhost"] else "false"
            instant_str = "true" if item["instant_bookable"] else "false"
            vec_str = f"[{', '.join(f'{x:.5f}' for x in item['vector'])}]"
            
            payload = (
                f"{{"
                f"id: {item['id']}, "
                f"text: '{item['text']}', "
                f"name: '{item['name']}', "
                f"neighbourhood: '{item['neighborhood']}', "
                f"location: {{lat: {item['lat']}, lon: {item['lon']}}}, "
                f"price: {item['price']}, "
                f"rating: {item['rating']}, "
                f"room_type: '{item['room_type']}', "
                f"accommodates: {item['accommodates']}, "
                f"superhost: {superhost_str}, "
                f"instant_bookable: {instant_str}, "
                f"reviews_count: {item['reviews_count']}, "
                f"vector: {{dense: {vec_str}}}"
                f"}}"
            )
            vals.append(payload)

        qql = f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)};"
        try:
            client.execute(pyqql.parse(qql))
            total_ingested += len(batch)
        except Exception as err:
            print(f"  Warning: batch error at index {i}: {err}")

        if (i + batch_size) % 500 == 0 or (i + batch_size) >= len(listings):
            print(f"  Progress: {min(i + batch_size, len(listings))}/{len(listings)} listings ingested...")

    print(f"\nSuccessfully ingested {total_ingested} Berlin Airbnb listings into '{config.COLLECTION}'.")
    count_res = client.execute(pyqql.parse(f"COUNT FROM {config.COLLECTION};"))
    print(f"Collection Count: {count_res}")

if __name__ == "__main__":
    main()
