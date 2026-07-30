#!/usr/bin/env python3
"""
Berlin Airbnb — QQL geo + hybrid showcase with turbo quantization + rescore.

Usage:
    python query.py              # parse-check
    python query.py --execute    # live against Qdrant
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import config

sys.path.insert(
    0, os.environ.get("QQL_LIB", str(Path(__file__).resolve().parents[2] / "crates" / "pyqql"))
)
import pyqql  # noqa: E402

C = config.COLLECTION
BG = config.BRANDENBURG_GATE
BB = config.MITTE_BBOX
POLY = config.KREUZBERG_POLYGON

# Turbo quant: always rescore with original vectors for quality
QPARAMS = (
    "PARAMS (hnsw_ef = 128, quantization = {ignore: false, rescore: true, oversampling: 2.0})"
)


def poly_literal() -> str:
    pts = ", ".join(f"{{lat: {p['lat']}, lon: {p['lon']}}}" for p in POLY)
    return f"[{pts}]"


QUERIES: list[tuple[str, str]] = [
    (
        "1. GEO_RADIUS — Brandenburg Gate, under €100 (dense + rescore)",
        f"""
        QUERY TEXT 'cozy studio near historic landmarks'
        FROM {C}
        USING dense
        WHERE location GEO_RADIUS {{
            center: {{lat: {BG['lat']}, lon: {BG['lon']}}},
            radius: 1500.0
          }}
          AND price <= 100.0
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "2. GEO_BBOX — Mitte + hybrid RRF + rescore",
        f"""
        QUERY TEXT 'spacious loft with balcony and fast wifi'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WHERE location GEO_BBOX {{
            top_left: {{lat: {BB['top_left']['lat']}, lon: {BB['top_left']['lon']}}},
            bottom_right: {{lat: {BB['bottom_right']['lat']}, lon: {BB['bottom_right']['lon']}}}
          }}
          AND room_type = 'Entire home apt'
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "3. GEO_POLYGON — Kreuzberg nightlife",
        f"""
        QUERY TEXT 'artistic flat nightlife and coffee shops'
        FROM {C}
        USING dense
        WHERE location GEO_POLYGON {{ exterior: {poly_literal()} }}
          AND rating >= 4.5
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "4. District filter (WHERE, not SHARD) — Mitte + high rating",
        f"""
        QUERY TEXT 'quiet apartment courtyard'
        FROM {C}
        USING dense
        WHERE district = 'Mitte' AND rating >= 4.5
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "5. Hybrid + superhost + price cap",
        f"""
        QUERY TEXT 'design apartment with garden'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WHERE superhost = true AND price <= 150.0
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "6. FORMULA — geo-decay from Brandenburg Gate",
        f"""
        WITH candidates AS (
          QUERY TEXT 'apartment near museums'
          FROM {C}
          USING dense
          {QPARAMS}
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
    ),
    (
        "7. ORDER BY price ASC",
        f"""
        QUERY ORDER BY price ASC FROM {C}
        WHERE rating >= 4.8 AND accommodates >= 2
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "8. SCROLL WITH VECTOR false",
        f"""
        SCROLL FROM {C}
        WHERE superhost = true
        WITH VECTOR false
        LIMIT 5
        """,
    ),
    (
        "9. COUNT exact — superhosts",
        f"""
        COUNT FROM {C}
        WHERE superhost = true
        WITH (exact = true)
        """,
    ),
    (
        "10. ACORN filtered dense + quant rescore",
        f"""
        QUERY TEXT 'family apartment near park'
        FROM {C}
        USING dense
        WHERE price <= 120.0 AND bedrooms >= 2
        PARAMS (
          acorn = true,
          max_selectivity = 0.4,
          hnsw_ef = 128,
          quantization = {{ignore: false, rescore: true, oversampling: 2.0}}
        )
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
    (
        "11. Quant oversampling 3× rescore (recall-focused)",
        f"""
        QUERY TEXT 'modern loft berlin center'
        FROM {C}
        USING dense
        WHERE price <= 200.0
        PARAMS (hnsw_ef = 256, quantization = {{ignore: false, rescore: true, oversampling: 3.0}})
        WITH PAYLOAD true
        LIMIT 8
        """,
    ),
    (
        "12. Hybrid DBSF fusion + rescore",
        f"""
        QUERY HYBRID TEXT 'sunny apartment balcony wifi'
        DENSE dense SPARSE sparse FUSION DBSF
        FROM {C}
        WHERE rating >= 4.6
        {QPARAMS}
        WITH PAYLOAD true
        LIMIT 5
        """,
    ),
]


def show_hits(label: str, data) -> None:
    print(f"\n═══ {label} ═══")
    if isinstance(data, dict):
        if "result" in data and isinstance(data["result"], dict) and "count" in data["result"]:
            print(f"  count = {data['result']['count']}")
            return
        points = data.get("result", data)
        if isinstance(points, dict):
            points = points.get("points") or points.get("hits") or []
    else:
        points = data
    if not isinstance(points, list):
        print(f"  {json.dumps(data)[:400]}")
        return
    if not points:
        print("  (no hits)")
        return
    for h in points[:5]:
        p = h.get("payload") or {}
        name = (p.get("name") or "")[:50]
        print(
            f"  s={h.get('score', 0):.4f}  €{p.get('price', '?')}  "
            f"{p.get('district', p.get('neighbourhood', '?'))}  "
            f"rating={p.get('rating', '?')}  | {name}"
        )


def main() -> int:
    ap = argparse.ArgumentParser(description="Berlin Airbnb QQL geo showcase")
    ap.add_argument("--execute", action="store_true", help="Run against Qdrant")
    args = ap.parse_args()

    print(f"pyqql {getattr(pyqql, '__version__', '?')}  collection={C}")
    print(f"embed={config.EMBED_MODEL}  quant=turbo/{config.QUANT_BITS} + rescore")
    print(f"mode={'execute' if args.execute else 'parse-only'}\n")

    client = None
    if args.execute:
        client = pyqql.Client(
            config.QDRANT_URL,
            embedder=pyqql.HttpEmbedder(
                config.EMBED_URL, config.EMBED_MODEL, config.EMBED_DIM
            ),
        )

    ok_n = 0
    for label, qql in QUERIES:
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
        if not args.execute:
            continue
        try:
            report = client.execute(pyqql.parse(qql)[0])
            data = report["results"][0].get("data", [])
            show_hits(label, data)
        except Exception as e:
            print(f"  ERROR: {e}")

    print(f"\n{ok_n}/{len(QUERIES)} statements valid.")
    if not args.execute:
        print("Re-run with --execute after ingest.py.")
    return 0 if ok_n == len(QUERIES) else 1


if __name__ == "__main__":
    sys.exit(main())
