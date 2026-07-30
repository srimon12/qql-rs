#!/usr/bin/env python3
"""
Berlin Airbnb — QQL geo + hybrid showcase.

Runs offline parse checks by default. Pass --execute to hit Qdrant
(after `python ingest.py`).

Highlights:
  GEO_RADIUS / GEO_BBOX / GEO_POLYGON
  hybrid USING HYBRID
  SHARD isolation by district
  inject_filter + inject_shard_key
  formula geo-decay
  COUNT / SCROLL / ORDER BY
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import config

sys.path.insert(0, os.environ.get("QQL_LIB", str(Path(__file__).resolve().parents[2] / "crates" / "pyqql")))
import pyqql  # noqa: E402

C = config.COLLECTION
BG = config.BRANDENBURG_GATE
BB = config.MITTE_BBOX
POLY = config.KREUZBERG_POLYGON


def poly_literal() -> str:
    pts = ", ".join(f"{{lat: {p['lat']}, lon: {p['lon']}}}" for p in POLY)
    return f"[{pts}]"


QUERIES: list[tuple[str, str, str | None]] = [
    (
        "1. GEO_RADIUS — Brandenburg Gate, under €100",
        f"""
        QUERY TEXT 'cozy studio near historic landmarks'
        FROM {C}
        USING dense
        WHERE location GEO_RADIUS {{
            center: {{lat: {BG['lat']}, lon: {BG['lon']}}},
            radius: 1500.0
          }}
          AND price <= 100.0
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
    (
        "2. GEO_BBOX — Mitte center, entire homes",
        f"""
        QUERY TEXT 'spacious loft with balcony and fast wifi'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WHERE location GEO_BBOX {{
            top_left: {{lat: {BB['top_left']['lat']}, lon: {BB['top_left']['lon']}}},
            bottom_right: {{lat: {BB['bottom_right']['lat']}, lon: {BB['bottom_right']['lon']}}}
          }}
          AND room_type = 'Entire home/apt'
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
    (
        "3. GEO_POLYGON — Kreuzberg nightlife district",
        f"""
        QUERY TEXT 'artistic flat nightlife and coffee shops'
        FROM {C}
        USING dense
        WHERE location GEO_POLYGON {{ exterior: {poly_literal()} }}
          AND rating >= 4.5
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
    (
        "4. SHARD 'mitte' — district isolation",
        f"""
        QUERY TEXT 'quiet apartment courtyard'
        FROM {C}
        USING dense
        WHERE rating >= 4.5
        WITH PAYLOAD true
        LIMIT 5
        """,
        "mitte",  # inject_shard_key
    ),
    (
        "5. SHARD 'kreuzberg' + superhost filter",
        f"""
        QUERY TEXT 'design apartment with garden'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WHERE superhost = true AND price <= 150.0
        WITH PAYLOAD true
        LIMIT 5
        """,
        "kreuzberg",
    ),
    (
        "6. FORMULA — score * geo decay from Brandenburg Gate",
        f"""
        WITH candidates AS (
          QUERY TEXT 'apartment near museums'
          FROM {C}
          USING dense
          LIMIT 50
        )
        QUERY FORMULA (
          score * GAUSS_DECAY(
            GEO_DISTANCE({BG['lat']}, {BG['lon']}, location),
            0.0, 3000.0, 0.5
          )
        ) DEFAULTS (location = {{lat: {BG['lat']}, lon: {BG['lon']}}})
        FROM {C}
        PREFETCH (candidates)
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
    (
        "7. ORDER BY price ASC (browse mode)",
        f"""
        QUERY ORDER BY price ASC FROM {C}
        WHERE rating >= 4.8 AND accommodates >= 2
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
    (
        "8. SCROLL WITH VECTOR false",
        f"""
        SCROLL FROM {C}
        WHERE superhost = true
        WITH VECTOR false
        LIMIT 5
        """,
        "mitte",
    ),
    (
        "9. COUNT exact — superhosts citywide",
        f"""
        COUNT FROM {C}
        WHERE superhost = true
        WITH (exact = true)
        """,
        None,
    ),
    (
        "10. ACORN filtered hybrid",
        f"""
        QUERY TEXT 'family apartment near park'
        FROM {C}
        USING dense
        WHERE price <= 120.0 AND bedrooms >= 2
        PARAMS (acorn = true, max_selectivity = 0.4, hnsw_ef = 128)
        WITH PAYLOAD true
        LIMIT 5
        """,
        None,
    ),
]


def show_hits(label: str, data) -> None:
    print(f"\n═══ {label} ═══")
    if isinstance(data, dict):
        # COUNT
        if "result" in data and isinstance(data["result"], dict) and "count" in data["result"]:
            print(f"  count = {data['result']['count']}")
            return
        points = data.get("result", data)
        if isinstance(points, dict):
            points = points.get("points") or points.get("hits") or []
    else:
        points = data
    if not isinstance(points, list):
        print(f"  {json.dumps(data)[:300]}")
        return
    for h in points[:5]:
        p = h.get("payload") or {}
        name = (p.get("name") or "")[:50]
        print(
            f"  s={h.get('score', 0):.4f}  €{p.get('price', '?')}  "
            f"{p.get('neighbourhood', '?')}  rating={p.get('rating', '?')}  | {name}"
        )


def main() -> int:
    ap = argparse.ArgumentParser(description="Berlin Airbnb QQL geo showcase")
    ap.add_argument("--execute", action="store_true", help="Run against Qdrant")
    ap.add_argument("--parse-only", action="store_true", help="Only validate QQL (default if no --execute)")
    args = ap.parse_args()
    execute = args.execute

    print(f"pyqql {pyqql.__version__}  collection={C}")
    print(f"mode={'execute' if execute else 'parse-only'}\n")

    client = None
    if execute:
        if config.EMBED_URL:
            url = config.EMBED_URL.rstrip("/")
            if not url.endswith("/embeddings"):
                url = f"{url}/v1/embeddings"
            client = pyqql.Client(
                config.QDRANT_URL,
                embedder=pyqql.HttpEmbedder(url, config.EMBED_MODEL, config.EMBED_DIM),
            )
        else:
            client = pyqql.Client(config.QDRANT_URL)

    ok_n = 0
    for label, qql, shard in QUERIES:
        valid = pyqql.is_valid(qql)
        status = "valid" if valid else "INVALID"
        print(f"[{status}] {label}")
        if not valid:
            try:
                pyqql.parse(qql)
            except Exception as e:
                print(f"  {e}")
            continue
        ok_n += 1

        if not execute:
            # Show inject path offline
            stmt = pyqql.parse(qql)[0]
            if shard:
                pyqql.inject_shard_key(stmt, shard)
                print(f"  inject_shard_key → {stmt.shard_key!r}")
            continue

        try:
            stmt = pyqql.parse(qql)[0]
            if shard:
                pyqql.inject_shard_key(stmt, shard)
                # Optional: also filter on district name loosely
                # pyqql.inject_filter(stmt, "district", "=", ...)  # district strings vary
            report = client.execute(stmt)
            data = report["results"][0].get("data", [])
            show_hits(label, data)
        except Exception as e:
            print(f"  ERROR: {e}")

    print(f"\n{ok_n}/{len(QUERIES)} statements valid.")
    if not execute:
        print("Re-run with --execute after ingest.py to query live data.")
    return 0 if ok_n == len(QUERIES) else 1


if __name__ == "__main__":
    sys.exit(main())
