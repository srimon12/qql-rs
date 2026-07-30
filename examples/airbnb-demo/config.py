"""
Configuration for Berlin Airbnb QQL Geo Showcase.
"""

from __future__ import annotations

import os

QDRANT_URL = os.getenv("QDRANT_URL", "http://localhost:6333")

# Ollama OpenAI-compatible embeddings (all-minilm:l6-v2 → 384-d)
EMBED_URL = os.getenv("EMBED_URL", "http://localhost:11434/v1/embeddings")
EMBED_MODEL = os.getenv("EMBED_MODEL", "all-minilm:l6-v2")
EMBED_DIM = int(os.getenv("EMBED_DIM", "384"))

COLLECTION = "berlin_airbnb"

# Cap for demo speed. Set 0 for all ~12.7k rows.
MAX_LISTINGS = int(os.getenv("MAX_LISTINGS", "1500"))

# Turbo binary quantization (2-bit is aggressive + fast; 4-bit is safer)
# Queries should set PARAMS (quantization = {rescore: true, …})
QUANT_BITS = int(os.getenv("QUANT_BITS", "2"))

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
