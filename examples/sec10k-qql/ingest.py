"""
Ingest SEC 10-K filings with auto-extracted metadata.

Every chunk gets payload fields for: tenant_id, fiscal_year, section
(Item 1A/7/8), detected risk categories, and financial figures.
This metadata enables rich QQL WHERE, GROUP BY, and ORDER BY queries.
"""

import sys, os, hashlib, html2text, requests, re
sys.path.insert(0, os.environ.get("QQL_LIB", "../../target/release"))
import pyqql
import config


def make_id(tenant, year, idx):
    h = hashlib.sha256(f"{tenant}_{year}_{idx}".encode()).digest()
    return int.from_bytes(h[:8], "big") & 0x7FFFFFFFFFFFFFFF


def extract_metadata(text: str) -> dict:
    """Pull structured metadata from a 10-K chunk."""
    meta = {}
    # Which section does this chunk belong to?
    sections = config.SECTION_RE.findall(text)  # search entire chunk
    if sections:
        meta["section"] = sections[0].strip().lower().replace(" ", "_")
    # Risk categories detected
    risks = set(m.group(0).lower().replace(" ", "_") for m in config.RISK_RE.finditer(text))
    if risks:
        meta["risks"] = ", ".join(sorted(risks)[:3])
    # Financial figures
    figures = config.REVENUE_RE.findall(text)
    if figures:
        meta["has_figures"] = True
    return meta


def ingest(client, tenant, year, url):
    print(f"  [{tenant}] FY{year}: downloading...")
    resp = requests.get(url, headers={"User-Agent": config.SEC_USER_AGENT}, timeout=120)
    resp.raise_for_status()
    text = html2text.HTML2Text()
    text.ignore_links = text.ignore_images = True
    text.body_width = 0
    raw = text.handle(resp.text)
    words = raw.split()
    chunks = [" ".join(words[i:i+config.CHUNK_SIZE])
              for i in range(0, len(words), config.CHUNK_SIZE - config.CHUNK_OVERLAP)]

    total = 0
    for batch_start in range(0, len(chunks), 50):
        batch = chunks[batch_start:batch_start+50]
        vals = []
        for i, chunk in enumerate(batch):
            pid = make_id(tenant, year, batch_start + i)
            safe = chunk.replace("\\", "\\\\").replace("'", "\\'")
            meta = extract_metadata(chunk)
            # Build payload: base fields + extracted metadata
            payload_parts = [
                f"id: {pid}",
                f"text: '{safe}'",
                f"tenant_id: '{tenant}'",
                f"company: '{tenant}'",
                f"fiscal_year: {year}",
            ]
            if "section" in meta:
                payload_parts.append(f"section: '{meta['section']}'")
            if "risks" in meta:
                payload_parts.append(f"risks: '{meta['risks']}'")
            if meta.get("has_figures"):
                payload_parts.append("has_figures: true")
            vals.append("{" + ", ".join(payload_parts) + "}")

        qql = (f"UPSERT INTO {config.COLLECTION} VALUES {', '.join(vals)} "
               f"USING DENSE MODEL '{config.EMBED_MODEL}' SHARD '{tenant}'")
        client.execute(pyqql.parse(qql))
        total += len(batch)
    print(f"  [{tenant}] FY{year}: {total} points → shard '{tenant}'")
    return total


def main():
    embedder = pyqql.HttpEmbedder(
        f"{config.LM_STUDIO}/v1/embeddings", config.EMBED_MODEL, config.EMBED_DIM)
    client = pyqql.Client(config.QDRANT_URL, embedder=embedder)

    # ── Setup: collection + indexes + shards ──
    try: client.execute(f"DROP COLLECTION {config.COLLECTION}")
    except: pass
    try: client.execute("DROP COLLECTION sec10k_qql_demo")
    except: pass
    client.execute(f"""
        CREATE COLLECTION {config.COLLECTION}
        HYBRID (dense VECTOR({config.EMBED_DIM}, COSINE), sparse SPARSE)
        WITH PARAMS (replication_factor=2, shard_number={len(config.TENANTS)*2},
                     sharding_method='custom')
    """)
    for stmt in pyqql.parse_all(
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR tenant_id TYPE keyword WITH (is_tenant=true);"
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR fiscal_year TYPE integer;"
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR section TYPE keyword;"
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR risks TYPE keyword;"
        f"CREATE INDEX ON COLLECTION {config.COLLECTION} FOR has_figures TYPE bool;"
    ):
        client.execute(stmt)
    for t in config.TENANTS:
        client.execute(f"CREATE SHARD '{t}' ON COLLECTION {config.COLLECTION}")

    # ── Ingest ──
    total = sum(ingest(client, t, y, u) for t, years in config.FILINGS.items() for y, u in years.items())
    print(f"\nIngested {total} chunks across {len(config.TENANTS)} tenants.")
    for t in config.TENANTS:
        s = pyqql.parse(f"COUNT FROM {config.COLLECTION}")
        pyqql.inject_filter(s, "tenant_id", "=", t)
        r = client.execute(s)
        print(f"  {t}: {r['data']['count']} points")


if __name__ == "__main__":
    main()
