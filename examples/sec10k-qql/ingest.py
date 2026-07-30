"""
Ingest SEC 10-K filings with auto-extracted metadata.

Every chunk gets payload fields for: tenant_id, fiscal_year, section
(Item 1A/7/8), detected risk categories, and financial figures.
This metadata enables rich QQL WHERE, GROUP BY, and ORDER BY queries.

Isolation layers:
  1. SHARD '<tenant>' on every UPSERT (physical)
  2. tenant_id payload + is_tenant index (logical)
"""

from __future__ import annotations

import hashlib
import html2text
import os
import re
import sys

import requests

# Prefer installed pyqql; fall back to local build path
sys.path.insert(0, os.environ.get("QQL_LIB", os.path.join(os.path.dirname(__file__), "../../crates/pyqql")))
import pyqql  # noqa: E402
import config  # noqa: E402


def make_id(tenant: str, year: int, idx: int) -> int:
    h = hashlib.sha256(f"{tenant}_{year}_{idx}".encode()).digest()
    return int.from_bytes(h[:8], "big") & 0x7FFFFFFFFFFFFFFF


def extract_metadata(text: str) -> dict:
    """Pull structured metadata from a 10-K chunk."""
    meta: dict = {}
    sections = config.SECTION_RE.findall(text)
    if sections:
        meta["section"] = sections[0].strip().lower().replace(" ", "_")
    risks = {m.group(0).lower().replace(" ", "_") for m in config.RISK_RE.finditer(text)}
    if risks:
        meta["risks"] = ", ".join(sorted(risks)[:3])
    if config.REVENUE_RE.search(text):
        meta["has_figures"] = True
    return meta


def escape_qql_str(s: str) -> str:
    return s.replace("\\", "\\\\").replace("'", "\\'")


def ingest(client, tenant: str, year: int, url: str) -> int:
    print(f"  [{tenant}] FY{year}: downloading...")
    resp = requests.get(url, headers={"User-Agent": config.SEC_USER_AGENT}, timeout=120)
    resp.raise_for_status()
    converter = html2text.HTML2Text()
    converter.ignore_links = converter.ignore_images = True
    converter.body_width = 0
    raw = converter.handle(resp.text)
    words = raw.split()
    step = config.CHUNK_SIZE - config.CHUNK_OVERLAP
    chunks = [
        " ".join(words[i : i + config.CHUNK_SIZE])
        for i in range(0, len(words), step)
    ]

    total = 0
    for batch_start in range(0, len(chunks), 50):
        batch = chunks[batch_start : batch_start + 50]
        vals = []
        for i, chunk in enumerate(batch):
            pid = make_id(tenant, year, batch_start + i)
            meta = extract_metadata(chunk)
            payload_parts = [
                f"id: {pid}",
                f"text: '{escape_qql_str(chunk)}'",
                f"tenant_id: '{tenant}'",
                f"company: '{tenant}'",
                f"fiscal_year: {year}",
            ]
            if "section" in meta:
                payload_parts.append(f"section: '{meta['section']}'")
            if "risks" in meta:
                payload_parts.append(f"risks: '{escape_qql_str(meta['risks'])}'")
            if meta.get("has_figures"):
                payload_parts.append("has_figures: true")
            vals.append("{" + ", ".join(payload_parts) + "}")

        # Auto-embed via HttpEmbedder: USING DENSE MODEL '…'
        # SHARD routes the batch to the tenant's physical shard.
        qql = (
            f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)} "
            f"USING DENSE MODEL '{config.EMBED_MODEL}' SHARD '{tenant}'"
        )
        client.execute(pyqql.parse(qql))
        total += len(batch)
    print(f"  [{tenant}] FY{year}: {total} points → shard '{tenant}'")
    return total


def main() -> None:
    embed_url = config.EMBED_URL
    embedder = pyqql.HttpEmbedder(embed_url, config.EMBED_MODEL, config.EMBED_DIM)
    client = pyqql.Client(config.QDRANT_URL, embedder=embedder)
    print(f"Embedder: {config.EMBED_MODEL} @ {embed_url} (dim={config.EMBED_DIM})")

    # ── Schema: hybrid + custom tenant sharding + turbo quant ──
    try:
        client.execute(f"DROP COLLECTION {config.COLLECTION}")
    except Exception:
        pass
    try:
        client.execute("DROP COLLECTION sec10k_qql_demo")
    except Exception:
        pass

    n_shards = max(len(config.TENANTS) * 2, 4)
    client.execute(f"""
        CREATE COLLECTION {config.COLLECTION}
        HYBRID (dense VECTOR({config.EMBED_DIM}, COSINE), sparse SPARSE)
        WITH HNSW (m = 16, ef_construct = 100, payload_m = 16)
        WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)
        WITH PARAMS (
            replication_factor = 1,
            shard_number = {n_shards},
            sharding_method = 'custom',
            shard_keys = [{", ".join(repr(t) for t in config.TENANTS)}]
        )
    """)

    index_script = "; ".join(
        [
            f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR tenant_id TYPE keyword WITH (is_tenant = true)",
            f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR fiscal_year TYPE integer",
            f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR section TYPE keyword",
            f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR risks TYPE keyword",
            f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR has_figures TYPE bool",
        ]
    )
    for stmt in pyqql.parse(index_script):
        client.execute(stmt)

    # Explicit shard keys (idempotent if WITH PARAMS already created them)
    for t in config.TENANTS:
        try:
            client.execute(
                f"CREATE SHARD KEY '{t}' ON COLLECTION {config.COLLECTION} WITH (shards_number = 2)"
            )
        except Exception as e:
            print(f"  shard key '{t}': {e}")

    # ── Ingest ──
    year_filter = None
    if config.DEMO_YEARS.strip():
        year_filter = {int(y.strip()) for y in config.DEMO_YEARS.split(",") if y.strip()}
        print(f"DEMO_YEARS filter: {sorted(year_filter)}")

    total = 0
    for t, years in config.FILINGS.items():
        for y, u in years.items():
            if year_filter is not None and y not in year_filter:
                continue
            total += ingest(client, t, y, u)
    print(f"\nIngested {total} chunks across {len(config.TENANTS)} tenants.")

    for t in config.TENANTS:
        # Prefer literal SHARD + WHERE so count body is always OpenAPI-clean
        r = client.execute(
            f"COUNT FROM {config.COLLECTION} WHERE tenant_id = '{t}' "
            f"SHARD '{t}' WITH (exact = true)"
        )
        count = r["results"][0]["data"]["result"]["count"]
        print(f"  {t}: {count} points")


if __name__ == "__main__":
    main()
