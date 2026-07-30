#!/usr/bin/env python3
"""
Ingest Berlin Airbnb listings into Qdrant via pyqql.

- Reads listings.csv or listings.csv.gz (Inside Airbnb)
- Hybrid collection: dense 384-d + sparse BM25-style vectors
- Custom sharding by Berlin district (SHARD 'mitte', 'kreuzberg', …)
- Geo + keyword + numeric payload indexes

Embedding modes:
  1. EMBED_URL set  → HttpEmbedder (real dense vectors; sparse still hashed)
  2. default        → deterministic hash dense + sparse (fully offline)
"""

from __future__ import annotations

import csv
import gzip
import hashlib
import math
import os
import re
import sys
import time
from collections import Counter
from pathlib import Path

import config

sys.path.insert(0, os.environ.get("QQL_LIB", str(Path(__file__).resolve().parents[2] / "crates" / "pyqql")))
import pyqql  # noqa: E402

ROOT = Path(__file__).resolve().parent


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


def escape_qql(s: str) -> str:
    return s.replace("\\", "\\\\").replace("'", "\\'")


def text_to_vector(text: str, dim: int = 384) -> list[float]:
    """Deterministic dense vector (offline fallback)."""
    vec = [0.0] * dim
    for word in text.lower().split():
        h = hashlib.sha256(word.encode()).digest()
        for i in range(dim):
            vec[i] += (h[i % len(h)] - 128) / 128.0
    norm = math.sqrt(sum(x * x for x in vec)) or 1.0
    return [round(x / norm, 5) for x in vec]


def text_to_sparse_vector(text: str) -> dict:
    """Sparse bag-of-words via token hashing (demo BM25-like)."""
    words = re.findall(r"\w+", text.lower())
    counts = Counter(words)
    index_map: dict[int, float] = {}
    for word, cnt in counts.items():
        h = int(hashlib.md5(word.encode()).hexdigest(), 16) % 100_000
        val = round(1.0 + math.log(cnt), 4)
        index_map[h] = max(index_map.get(h, 0.0), val)
    keys = sorted(index_map)
    return {"indices": keys, "values": [index_map[k] for k in keys]}


def open_listings():
    csv_gz = ROOT / "listings.csv.gz"
    csv_plain = ROOT / "listings.csv"
    if csv_plain.exists():
        return open(csv_plain, "r", encoding="utf-8", newline="")
    if csv_gz.exists():
        return gzip.open(csv_gz, "rt", encoding="utf-8", newline="")
    print(f"Error: need {csv_plain.name} or {csv_gz.name} in {ROOT}")
    sys.exit(1)


def load_listings() -> list[dict]:
    listings = []
    with open_listings() as f:
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
                text = f"{name}. {desc} {overview}".strip() or f"Airbnb listing {lid}"

                neighbourhood = clean_string(
                    row.get("neighbourhood_cleansed")
                    or row.get("neighbourhood_group_cleansed")
                    or "Mitte"
                )
                district = clean_string(row.get("neighbourhood_group_cleansed") or "Mitte")
                shard = config.shard_for_district(district)

                listings.append(
                    {
                        "id": lid,
                        "text": text[:350],
                        "name": name[:100],
                        "neighbourhood": neighbourhood,
                        "district": district,
                        "shard": shard,
                        "property_type": clean_string(row.get("property_type") or "Entire rental unit"),
                        "room_type": clean_string(row.get("room_type") or "Entire home apt"),
                        "host_name": clean_string(row.get("host_name") or "Host")[:50],
                        "lat": lat,
                        "lon": lon,
                        "price": clean_price(row.get("price") or ""),
                        "accommodates": int(float(row.get("accommodates") or 2)),
                        "bedrooms": int(float(row.get("bedrooms") or 1)),
                        "beds": int(float(row.get("beds") or 1)),
                        "minimum_nights": int(float(row.get("minimum_nights") or 1)),
                        "superhost": (row.get("host_is_superhost") or "").lower() == "t",
                        "instant_bookable": (row.get("instant_bookable") or "").lower() == "t",
                        "reviews_count": int(float(row.get("number_of_reviews") or 0)),
                        "rating": float(row.get("review_scores_rating") or 4.5),
                        "rating_location": float(row.get("review_scores_location") or 4.5),
                        "vector": text_to_vector(text, config.EMBED_DIM),
                        "sparse": text_to_sparse_vector(text),
                    }
                )
                if config.MAX_LISTINGS and len(listings) >= config.MAX_LISTINGS:
                    break
            except Exception:
                continue
    return listings


def main() -> None:
    t0 = time.time()
    print("Loading Berlin Airbnb dataset…")
    listings = load_listings()
    print(f"Loaded {len(listings)} listings in {time.time() - t0:.2f}s")

    use_http = bool(config.EMBED_URL)
    if use_http:
        url = config.EMBED_URL.rstrip("/")
        if not url.endswith("/embeddings"):
            url = f"{url}/v1/embeddings"
        embedder = pyqql.HttpEmbedder(url, config.EMBED_MODEL, config.EMBED_DIM)
        client = pyqql.Client(config.QDRANT_URL, embedder=embedder)
        print(f"Embedder: HTTP {url}")
    else:
        client = pyqql.Client(config.QDRANT_URL)
        print("Embedder: offline hash vectors (set EMBED_URL for real dense)")

    print(f"Setting up collection '{config.COLLECTION}'…")
    try:
        client.execute(f"DROP COLLECTION {config.COLLECTION}")
    except Exception:
        pass

    shard_list = ", ".join(repr(k) for k in config.SHARD_KEYS)
    client.execute(
        f"""
        CREATE COLLECTION {config.COLLECTION}
        (dense VECTOR({config.EMBED_DIM}, COSINE), sparse SPARSE)
        WITH PARAMS (
            shard_number = {max(len(config.SHARD_KEYS) * 2, 4)},
            sharding_method = 'custom',
            shard_keys = [{shard_list}]
        )
        """
    )

    indexes = [
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR location TYPE geo",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR neighbourhood TYPE keyword",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR district TYPE keyword WITH (is_tenant = true)",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR price TYPE float",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR rating TYPE float",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR rating_location TYPE float",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR room_type TYPE keyword",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR property_type TYPE keyword",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR accommodates TYPE integer",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR bedrooms TYPE integer",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR superhost TYPE bool",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR instant_bookable TYPE bool",
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR reviews_count TYPE integer",
    ]
    for q in indexes:
        try:
            for stmt in pyqql.parse(q):
                client.execute(stmt)
        except Exception as e:
            print(f"  index notice: {e}")

    for key in config.SHARD_KEYS:
        try:
            client.execute(
                f"CREATE SHARD KEY '{key}' ON COLLECTION {config.COLLECTION} WITH (shards_number = 1)"
            )
        except Exception as e:
            print(f"  shard '{key}': {e}")

    print(f"Ingesting {len(listings)} listings…")
    batch_size = 40
    for i in range(0, len(listings), batch_size):
        batch = listings[i : i + batch_size]
        # Group by shard so each UPSERT carries one SHARD key
        by_shard: dict[str, list] = {}
        for item in batch:
            by_shard.setdefault(item["shard"], []).append(item)

        for shard, items in by_shard.items():
            vals = []
            for item in items:
                sp = item["sparse"]
                if use_http:
                    # Auto-embed dense from text; still pass sparse explicitly
                    payload = (
                        "{"
                        f"id: {item['id']}, "
                        f"text: '{escape_qql(item['text'])}', "
                        f"name: '{escape_qql(item['name'])}', "
                        f"neighbourhood: '{escape_qql(item['neighbourhood'])}', "
                        f"district: '{escape_qql(item['district'])}', "
                        f"property_type: '{escape_qql(item['property_type'])}', "
                        f"room_type: '{escape_qql(item['room_type'])}', "
                        f"host_name: '{escape_qql(item['host_name'])}', "
                        f"location: {{lat: {item['lat']}, lon: {item['lon']}}}, "
                        f"price: {item['price']}, "
                        f"accommodates: {item['accommodates']}, "
                        f"bedrooms: {item['bedrooms']}, "
                        f"beds: {item['beds']}, "
                        f"minimum_nights: {item['minimum_nights']}, "
                        f"rating: {item['rating']}, "
                        f"rating_location: {item['rating_location']}, "
                        f"superhost: {'true' if item['superhost'] else 'false'}, "
                        f"instant_bookable: {'true' if item['instant_bookable'] else 'false'}, "
                        f"reviews_count: {item['reviews_count']}"
                        "}"
                    )
                else:
                    dense_str = f"[{', '.join(f'{x:.5f}' for x in item['vector'])}]"
                    payload = (
                        "{"
                        f"id: {item['id']}, "
                        f"text: '{escape_qql(item['text'])}', "
                        f"name: '{escape_qql(item['name'])}', "
                        f"neighbourhood: '{escape_qql(item['neighbourhood'])}', "
                        f"district: '{escape_qql(item['district'])}', "
                        f"property_type: '{escape_qql(item['property_type'])}', "
                        f"room_type: '{escape_qql(item['room_type'])}', "
                        f"host_name: '{escape_qql(item['host_name'])}', "
                        f"location: {{lat: {item['lat']}, lon: {item['lon']}}}, "
                        f"price: {item['price']}, "
                        f"accommodates: {item['accommodates']}, "
                        f"bedrooms: {item['bedrooms']}, "
                        f"beds: {item['beds']}, "
                        f"minimum_nights: {item['minimum_nights']}, "
                        f"rating: {item['rating']}, "
                        f"rating_location: {item['rating_location']}, "
                        f"superhost: {'true' if item['superhost'] else 'false'}, "
                        f"instant_bookable: {'true' if item['instant_bookable'] else 'false'}, "
                        f"reviews_count: {item['reviews_count']}, "
                        f"vector: {{dense: {dense_str}, sparse: {{indices: {sp['indices']}, values: {sp['values']}}}}}"
                        "}"
                    )
                vals.append(payload)

            if use_http:
                qql = (
                    f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)} "
                    f"USING DENSE MODEL '{config.EMBED_MODEL}' SHARD '{shard}'"
                )
            else:
                qql = f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)} SHARD '{shard}'"

            try:
                client.execute(pyqql.parse(qql))
            except Exception as e:
                print(f"  batch error shard={shard} idx={i}: {e}")

        done = min(i + batch_size, len(listings))
        if done % 500 < batch_size or done == len(listings):
            print(f"  Progress: {done}/{len(listings)}")

    cnt = client.execute(pyqql.parse(f"COUNT FROM {config.COLLECTION} WITH (exact = true)"))
    print(f"\nDone. Collection count response: {cnt}")
    print(f"Shards: {', '.join(config.SHARD_KEYS)}")


if __name__ == "__main__":
    main()
