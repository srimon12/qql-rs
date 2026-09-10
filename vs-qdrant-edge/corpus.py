"""Deterministic corpus loader — re-uses the `vs-qdrant` artifacts.

Nothing is generated or embedded here: the JSONL docs, f32 dense/colbert
matrices, BM25 postings and fixed query vectors all come from
`../vs-qdrant/data` (SHA256-manifested there by pipeline/embed.py).
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np

from config import COLBERT_DIM, DATA


def _jsonl(name: str) -> list[dict]:
    with (DATA / name).open(encoding="utf-8") as handle:
        return [json.loads(line) for line in handle]


def _sparse(name: str) -> list[dict]:
    return json.loads((DATA / name).read_text(encoding="utf-8"))


def _f32(name: str) -> np.ndarray:
    return np.fromfile(DATA / name, dtype=np.float32)


def _colbert_queries(entry: dict) -> list[list[float]]:
    """Normalize a fixed colbert query to list-of-token-vectors."""
    q = entry["colbert"]
    if q and isinstance(q[0], list):
        return q
    assert len(q) % COLBERT_DIM == 0, "colbert query not divisible by 128"
    return [q[i : i + COLBERT_DIM] for i in range(0, len(q), COLBERT_DIM)]


def load() -> dict:
    """Load everything the harness needs, as plain Python / numpy values."""
    queries = json.loads((DATA / "queries.json").read_text(encoding="utf-8"))
    return {
        "docs": {
            "berlin": _jsonl("berlin.jsonl"),
            "legal": _jsonl("legal.jsonl"),
        },
        "dense": {
            "berlin": _f32("berlin_dense.f32").reshape(-1, 384),
            "legal": _f32("legal_dense.f32").reshape(-1, 384),
        },
        "sparse": {
            "berlin": _sparse("berlin_bm25.json"),
            "legal": _sparse("legal_bm25.json"),
        },
        "colbert": {
            "flat": _f32("legal_colbert.f32"),
            "lens": json.loads(
                (DATA / "legal_colbert_lens.json").read_text(encoding="utf-8")
            ),
        },
        "queries": {
            "berlin": [
                {
                    "text": q["text"],
                    "dense": q["dense"],
                    "sparse": q["sparse"],
                }
                for q in queries["berlin"]
            ],
            "legal": [
                {
                    "text": q["text"],
                    "dense": q["dense"],
                    "sparse": q["sparse"],
                    "colbert": _colbert_queries(q),
                }
                for q in queries["legal"]
            ],
        },
    }


def colbert_offsets(lens: list[int]) -> list[int]:
    offsets = [0]
    for n in lens:
        offsets.append(offsets[-1] + n * COLBERT_DIM)
    return offsets


def data_dir() -> Path:
    assert DATA.is_dir(), f"missing vs-qdrant data dir: {DATA}"
    return DATA
