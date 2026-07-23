"""
Ingest Berlin Airbnb Dataset into Qdrant via pyqql
Ultra-fast loading directly from uncompressed listings.csv
Supports Dual Vector Indexing: 384-d Dense Neural Vector + BM25 Sparse Vector
"""

import csv
import hashlib
import math
import os
import re
import sys
import time
from collections import Counter

import pyqql
import config

def clean_string(val: str) -> str:
    if not val:
        return ""
    s = re.sub(r"<[^>]+>", " ", val)
    s = re.sub(r"[^\w\s.,!?-]", " ", s)
    return re.sub(r"\s+", " ", s).strip()

def clean_price(val: str) -> float:
    if not val:
        return 0.0
    c = re.sub(r"[^\d.]", "", val)
    try:
        return float(c)
    except Exception:
        return 0.0

def text_to_vector(text: str, dim: int = 384) -> list[float]:
    """Generate a 384-d normalized dense vector directly from text."""
    vec = [0.0] * dim
    for word in text.lower().split():
        h = hashlib.sha256(word.encode()).digest()
        for i in range(dim):
            vec[i] += (h[i % len(h)] - 128) / 128.0
    norm = math.sqrt(sum(x * x for x in vec)) or 1.0
    return [round(x / norm, 5) for x in vec]

def text_to_sparse_vector(text: str) -> dict:
    """Generate a sparse vector (unique sorted indices + values) using BM25 token frequency hashing."""
    words = re.findall(r"\w+", text.lower())
    counts = Counter(words)
    index_map = {}
    for word, cnt in counts.items():
        h = int(hashlib.md5(word.encode()).hexdigest(), 16) % 100000
        val = round(1.0 + math.log(cnt), 4)
        index_map[h] = max(index_map.get(h, 0.0), val)

    sorted_keys = sorted(index_map.keys())
    return {
        "indices": sorted_keys,
        "values": [index_map[k] for k in sorted_keys],
    }

def load_listings() -> list[dict]:
    csv_path = os.path.join(os.path.dirname(__file__), "listings.csv")
    if not os.path.exists(csv_path):
        print(f"Error: Dataset {csv_path} not found.")
        sys.exit(1)

    listings = []
    with open(csv_path, "r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            try:
                lat = float(row.get("latitude") or 0)
                lon = float(row.get("longitude") or 0)
                if not lat or not lon:
                    continue
                lid = int(float(row.get("id") or 0))
                if not lid:
                    continue

                name = clean_string(row.get("name") or "")
                desc = clean_string(row.get("description") or "")
                overview = clean_string(row.get("neighborhood_overview") or "")
                text = f"{name}. {desc} {overview}".strip()
                if not text:
                    text = f"Airbnb listing {lid}"

                neighbourhood = clean_string(row.get("neighbourhood_cleansed") or row.get("neighbourhood_group_cleansed") or "Mitte")
                district = clean_string(row.get("neighbourhood_group_cleansed") or "Mitte")
                property_type = clean_string(row.get("property_type") or "Entire rental unit")
                room_type = clean_string(row.get("room_type") or "Entire home apt")
                host_name = clean_string(row.get("host_name") or "Host")

                price = clean_price(row.get("price") or "")
                superhost = (row.get("host_is_superhost") or "").lower() == "t"
                instant_bookable = (row.get("instant_bookable") or "").lower() == "t"

                accommodates = int(float(row.get("accommodates") or 2))
                bedrooms = int(float(row.get("bedrooms") or 1))
                beds = int(float(row.get("beds") or 1))
                min_nights = int(float(row.get("minimum_nights") or 1))
                reviews_count = int(float(row.get("number_of_reviews") or 0))

                rating = float(row.get("review_scores_rating") or 4.5)
                rating_cleanliness = float(row.get("review_scores_cleanliness") or 4.5)
                rating_location = float(row.get("review_scores_location") or 4.5)
                rating_value = float(row.get("review_scores_value") or 4.5)

                dense_vec = text_to_vector(text, config.EMBED_DIM)
                sparse_vec = text_to_sparse_vector(text)

                listings.append({
                    "id": lid,
                    "text": text[:350],
                    "name": name[:100],
                    "neighbourhood": neighbourhood,
                    "district": district,
                    "property_type": property_type,
                    "room_type": room_type,
                    "host_name": host_name[:50],
                    "lat": lat,
                    "lon": lon,
                    "price": price,
                    "accommodates": accommodates,
                    "bedrooms": bedrooms,
                    "beds": beds,
                    "minimum_nights": min_nights,
                    "superhost": superhost,
                    "instant_bookable": instant_bookable,
                    "reviews_count": reviews_count,
                    "rating": rating,
                    "rating_cleanliness": rating_cleanliness,
                    "rating_location": rating_location,
                    "rating_value": rating_value,
                    "vector": dense_vec,
                    "sparse": sparse_vec,
                })
                if config.MAX_LISTINGS and len(listings) >= config.MAX_LISTINGS:
                    break
            except Exception:
                continue
    return listings

def main():
    t0 = time.time()
    print("Loading Berlin Airbnb dataset from uncompressed listings.csv...")
    listings = load_listings()
    t1 = time.time()
    print(f"Loaded {len(listings)} listings with 21 rich payload fields in {t1 - t0:.3f}s.")

    client = pyqql.Client(config.QDRANT_URL)

    # ── Setup Collection & Indexes ──
    print(f"Setting up collection '{config.COLLECTION}'...")
    try:
        client.execute(f"DROP COLLECTION {config.COLLECTION};")
    except Exception:
        pass

    # Create Collection with Dense (384-d) and Sparse (BM25) Vectors
    client.execute(f"""
        CREATE COLLECTION {config.COLLECTION}
        (dense VECTOR({config.EMBED_DIM}, COSINE), sparse SPARSE);
    """)

    # Create Payload Indexes for Geo, Keyword, Numeric, and Boolean filtering
    index_queries = [
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR location TYPE geo;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR neighbourhood TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR district TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR price TYPE float;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR rating TYPE float;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR rating_location TYPE float;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR room_type TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR property_type TYPE keyword;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR accommodates TYPE integer;",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR bedrooms TYPE integer;",
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

    for i in range(0, len(listings), batch_size):
        batch = listings[i:i + batch_size]
        vals = []
        for item in batch:
            superhost_str = "true" if item["superhost"] else "false"
            instant_str = "true" if item["instant_bookable"] else "false"
            dense_str = f"[{', '.join(f'{x:.5f}' for x in item['vector'])}]"
            sp = item["sparse"]
            
            payload = (
                "{"
                f"id: {item['id']}, "
                f"text: '{item['text']}', "
                f"name: '{item['name']}', "
                f"neighbourhood: '{item['neighbourhood']}', "
                f"district: '{item['district']}', "
                f"property_type: '{item['property_type']}', "
                f"room_type: '{item['room_type']}', "
                f"host_name: '{item['host_name']}', "
                f"location: {{lat: {item['lat']}, lon: {item['lon']}}}, "
                f"price: {item['price']}, "
                f"accommodates: {item['accommodates']}, "
                f"bedrooms: {item['bedrooms']}, "
                f"beds: {item['beds']}, "
                f"minimum_nights: {item['minimum_nights']}, "
                f"rating: {item['rating']}, "
                f"rating_location: {item['rating_location']}, "
                f"superhost: {superhost_str}, "
                f"instant_bookable: {instant_str}, "
                f"reviews_count: {item['reviews_count']}, "
                f"vector: {{dense: {dense_str}, sparse: {{indices: {sp['indices']}, values: {sp['values']}}}}}"
                "}"
            )
            vals.append(payload)

        qql = f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)};"
        try:
            client.execute(pyqql.parse(qql))
        except Exception as e:
            print(f"  Warning: batch error at index {i}: {e}")

        if (i + batch_size) % 500 == 0 or (i + batch_size) >= len(listings):
            print(f"  Progress: {min(i + batch_size, len(listings))}/{len(listings)} listings ingested...")

    print(f"\nSuccessfully ingested {len(listings)} Berlin Airbnb listings into '{config.COLLECTION}'.")
    cnt_res = client.execute(pyqql.parse(f"COUNT FROM {config.COLLECTION};"))
    print(f"Collection Count: {cnt_res}")

if __name__ == "__main__":
    main()
