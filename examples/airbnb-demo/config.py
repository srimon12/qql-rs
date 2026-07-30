"""
Configuration for Berlin Airbnb QQL Geo Showcase.
"""

from __future__ import annotations

import os
import re

QDRANT_URL = os.getenv("QDRANT_URL", "http://localhost:6333")
# Optional real embeddings; if unset, ingest uses deterministic hash vectors
EMBED_URL = os.getenv("EMBED_URL", "")
EMBED_MODEL = os.getenv("EMBED_MODEL", "all-MiniLM-L6-v2")
EMBED_DIM = int(os.getenv("EMBED_DIM", "384"))

COLLECTION = "berlin_airbnb"

# Cap for demo speed (full Berlin set is ~12.7k). Set 0 for all.
MAX_LISTINGS = int(os.getenv("MAX_LISTINGS", "2500"))

# Map neighbourhood_group → short shard key (custom sharding)
# Keys must be lowercase alphanumeric / underscore for clean QQL literals.
DISTRICT_SHARDS = {
    "mitte": "mitte",
    "pankow": "pankow",
    "friedrichshain-kreuzberg": "kreuzberg",
    "neukolln": "neukolln",
    "neukölln": "neukolln",
    "charlottenburg-wilmersdorf": "charlottenburg",
    "tempelhof-schoneberg": "tempelhof",
    "tempelhof-schöneberg": "tempelhof",
    "treptow-kopenick": "treptow",
    "treptow-köpenick": "treptow",
    "steglitz-zehlendorf": "steglitz",
    "lichtenberg": "lichtenberg",
    "reinickendorf": "reinickendorf",
    "spandau": "spandau",
    "marzahn-hellersdorf": "marzahn",
}


def shard_for_district(district: str) -> str:
    key = re.sub(r"\s+", "-", district.strip().lower())
    key = key.replace("ö", "o").replace("ü", "u").replace("ä", "a")
    return DISTRICT_SHARDS.get(key, "other")


SHARD_KEYS = sorted(set(DISTRICT_SHARDS.values()) | {"other"})

# Berlin landmarks for geo demos
BRANDENBURG_GATE = {"lat": 52.5163, "lon": 13.3777}
MITTE_BBOX = {
    "top_left": {"lat": 52.535, "lon": 13.360},
    "bottom_right": {"lat": 52.505, "lon": 13.420},
}
# Approximate Kreuzberg nightlife polygon (closed ring)
KREUZBERG_POLYGON = [
    {"lat": 52.500, "lon": 13.370},
    {"lat": 52.515, "lon": 13.430},
    {"lat": 52.485, "lon": 13.450},
    {"lat": 52.470, "lon": 13.390},
    {"lat": 52.500, "lon": 13.370},
]
