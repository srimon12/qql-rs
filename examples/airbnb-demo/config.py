"""
Configuration for Berlin Airbnb QQL Demo Ingestion
"""

import os

# Qdrant & Embedding Endpoint Configuration
QDRANT_URL = os.getenv("QDRANT_URL", "http://localhost:6333")
LM_STUDIO = os.getenv("EMBED_URL", "http://localhost:1234")
EMBED_MODEL = os.getenv("EMBED_MODEL", "all-MiniLM-L6-v2")
EMBED_DIM = 384

COLLECTION = "berlin_airbnb"

# Max listings to ingest per batch / total limit for demo (None for all 12.7k)
MAX_LISTINGS = int(os.getenv("MAX_LISTINGS", "2500"))
