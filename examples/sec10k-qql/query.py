"""
QQL Feature Showcase — major capabilities against real SEC 10-K data.

Tenant isolation is applied at the host layer (never string-built into the query):
  inject_filter(stmt, "tenant_id", "=", tenant)   # logical
  SHARD in QQL or stmt.shard_key = tenant         # physical

Demonstrates QQL 1.2:
  USING HYBRID shorthand, COUNT WITH (exact), SCROLL WITH VECTOR,
  PARAMS (acorn / timeout / consistency), formula, CTE fusion, GROUP BY, ORDER BY.
"""

from __future__ import annotations

import os
import sys

import requests

sys.path.insert(0, os.environ.get("QQL_LIB", os.path.join(os.path.dirname(__file__), "../../crates/pyqql")))
import pyqql  # noqa: E402
import config  # noqa: E402

C = config.COLLECTION


def client():
    e = pyqql.HttpEmbedder(config.EMBED_URL, config.EMBED_MODEL, config.EMBED_DIM)
    return pyqql.Client(config.QDRANT_URL, embedder=e)


# Turbo quant rescore defaults for quality under quantization
QRESCORE = "PARAMS (hnsw_ef = 128, quantization = {ignore: false, rescore: true, oversampling: 2.0})"


def secure(qql_stmt: str, tenant: str | None = None):
    """Parse + optional tenant isolation (filter + shard)."""
    stmt = pyqql.parse(qql_stmt)[0]
    if tenant:
        pyqql.inject_filter(stmt, "tenant_id", "=", tenant)
        stmt.shard_key = tenant
    return stmt


def run(qql_stmt: str, tenant: str | None = None):
    report = client().execute(secure(qql_stmt, tenant))
    return report["results"][0].get("data", [])


def show(label: str, hits, n: int = 5):
    if isinstance(hits, dict):
        # Some endpoints return {result: {points: [...]}} etc.
        hits = hits.get("result", hits)
        if isinstance(hits, dict):
            hits = hits.get("points") or hits.get("hits") or []
    if not isinstance(hits, list):
        print(f"\n═══ {label} ═══\n  {hits}")
        return
    print(f"\n═══ {label} ({len(hits)} hits) ═══")
    for h in hits[:n]:
        p = h.get("payload", {}) or {}
        t, y = p.get("tenant_id", "?"), p.get("fiscal_year", "?")
        text = (h.get("text") or p.get("text") or "")[:110].replace("\n", " ")
        print(f"  s={h.get('score', 0):.4f} {t} FY{y} | {text}")


def llm(question: str, hits) -> str:
    if isinstance(hits, dict):
        hits = hits.get("result", hits)
        if isinstance(hits, dict):
            hits = hits.get("points") or hits.get("hits") or []
    ctx = "\n".join(f"[{i}] {(h.get('text') or '')[:400]}" for i, h in enumerate(hits[:3], 1))
    prompt = (
        f"Context:\n{ctx}\n\nQuestion: {question}\n"
        f"Answer concisely with specific facts:"
    )
    base = config.LLM_BASE.rstrip("/")
    # Prefer OpenAI-compatible chat; fall back to Ollama /api/chat
    try:
        r = requests.post(
            f"{base}/v1/chat/completions",
            json={
                "model": config.LLM_MODEL,
                "messages": [{"role": "user", "content": prompt}],
            },
            timeout=120,
        )
        if r.ok:
            return r.json()["choices"][0]["message"]["content"].strip()
    except Exception:
        pass
    r = requests.post(
        "http://localhost:11434/api/chat",
        json={
            "model": config.LLM_MODEL,
            "messages": [{"role": "user", "content": prompt}],
            "stream": False,
        },
        timeout=120,
    )
    return r.json().get("message", {}).get("content", "").strip()


def ok(qql_stmt: str) -> bool:
    try:
        pyqql.parse(qql_stmt)
        return True
    except Exception as e:
        print(f"  PARSE ERROR: {e}")
        return False


# ═══════════════════════════════════════════════════════════════════
# Query catalog — every template is valid QQL 1.2
# ═══════════════════════════════════════════════════════════════════

queries = [
    (
        "1. HYBRID RRF (front-form) + quant rescore",
        f"""
        QUERY HYBRID TEXT 'cybersecurity risk factors' DENSE dense SPARSE sparse FUSION RRF
        FROM {C}
        {QRESCORE}
        WITH PAYLOAD true LIMIT 5
        """,
        "honeywell",
    ),
    (
        "2. HYBRID shorthand (USING HYBRID) + rescore",
        f"""
        QUERY TEXT 'supply chain disruption'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        {QRESCORE}
        WITH PAYLOAD true LIMIT 5
        """,
        "honeywell",
    ),
    (
        "3. HYBRID DBSF + rescore",
        f"""
        QUERY HYBRID TEXT 'supply chain disruption' DENSE dense SPARSE sparse FUSION DBSF
        FROM {C}
        {QRESCORE}
        WITH PAYLOAD true LIMIT 5
        """,
        "honeywell",
    ),
    (
        "4. CTE + PREFETCH + FUSION",
        f"""
        WITH a AS (QUERY TEXT 'supply chain risk' FROM {C} USING dense LIMIT 100),
             b AS (QUERY TEXT 'supply chain risk' FROM {C} USING sparse LIMIT 100)
        QUERY FUSION RRF FROM {C}
        PREFETCH (a, b)
        {QRESCORE}
        WITH PAYLOAD true LIMIT 5
        """,
        "honeywell",
    ),
    (
        "5. PREFETCH + SCORE THRESHOLD",
        f"""
        QUERY NEAREST TEXT 'missile defense contract' FROM {C} USING dense
        PREFETCH (QUERY TEXT 'missile defense' FROM {C} USING sparse LIMIT 50)
        SCORE THRESHOLD 0.3
        {QRESCORE}
        WITH PAYLOAD true LIMIT 5
        """,
        "rtx",
    ),
    (
        "6. MMR diversification",
        f"""
        QUERY MMR TEXT 'manufacturing operations' DIVERSITY 0.5 CANDIDATES 100
        FROM {C} USING dense
        PARAMS (hnsw_ef = 256, quantization = {{ignore: false, rescore: true, oversampling: 2.0}})
        WITH PAYLOAD true LIMIT 5
        """,
        "3m",
    ),
    (
        "7. FORMULA score shaping",
        f"""
        WITH candidates AS (QUERY TEXT 'financial results revenue' FROM {C} USING dense LIMIT 30)
        QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0)
        FROM {C}
        PREFETCH (candidates)
        WITH PAYLOAD true LIMIT 5
        """,
        "rtx",
    ),
    (
        "8. ACORN filtered search + rescore",
        f"""
        QUERY TEXT 'risk factors'
        FROM {C}
        USING dense
        WHERE fiscal_year >= 2024
        PARAMS (acorn = true, max_selectivity = 0.5, hnsw_ef = 128,
                quantization = {{ignore: false, rescore: true, oversampling: 2.0}})
        WITH PAYLOAD true LIMIT 5
        """,
        "ge",
    ),
    (
        "9. Request timeout + consistency",
        f"""
        QUERY TEXT 'aerospace defense contracts'
        FROM {C}
        USING dense
        PARAMS (timeout = 30, consistency = majority,
                quantization = {{rescore: true, oversampling: 2.0}})
        WITH PAYLOAD true LIMIT 5
        """,
        "rtx",
    ),
    (
        "10. SCROLL WITH VECTOR false",
        f"""
        SCROLL FROM {C} WITH VECTOR false LIMIT 3
        """,
        "honeywell",
    ),
    (
        "11. ORDER BY fiscal_year",
        f"""
        QUERY ORDER BY fiscal_year DESC FROM {C}
        WHERE fiscal_year >= 2023
        WITH PAYLOAD true LIMIT 5
        """,
        "ge",
    ),
]

print(f"pyqql {pyqql.__version__}  collection={C}\n")

for label, template, tenant in queries:
    if not ok(template):
        continue
    try:
        show(label, run(template, tenant=tenant))
    except Exception as e:
        print(f"\n═══ {label} ═══\n  ERROR: {e}")

# ── GROUP BY ──
print("\n═══ 12. GROUP BY fiscal_year ═══")
try:
    stmt = secure(
        f"""
        QUERY TEXT 'financial results' FROM {C} USING dense
        GROUP BY fiscal_year SIZE 3 LIMIT 20
        """,
        tenant="rtx",
    )
    resp = client().execute(stmt)
    data = resp["results"][0].get("data", {})
    groups = data.get("result", {}).get("groups", []) or data.get("groups", [])
    print(f"  ({len(groups)} groups)")
    for g in groups[:5]:
        print(f"  group={g.get('id', '?')}  hits={len(g.get('hits', []))}")
except Exception as e:
    print(f"  ERROR: {e}")

# ── COUNT exact ──
print("\n═══ 13. COUNT WITH (exact = true) ═══")
for t in config.TENANTS:
    try:
        s = secure(f"COUNT FROM {C} WITH (exact = true)", tenant=t)
        all_ = client().execute(s)["results"][0]["data"]["result"]["count"]
        s2 = secure(f"COUNT FROM {C} WHERE has_figures = true WITH (exact = true)", tenant=t)
        fig_ = client().execute(s2)["results"][0]["data"]["result"]["count"]
        print(f"  {t:12s}: {all_:>5} total | {fig_:>5} with financial figures")
    except Exception as e:
        print(f"  {t}: ERROR {e}")

# ── Isolation proof (filter + shard, no SHARD literal in source) ──
print("\n═══ 14. ISOLATION PROOF (inject_filter + SHARD / shard_key) ═══")
for tenant in ["honeywell", "rtx"]:
    try:
        hits = run(
            f"""
            QUERY TEXT 'Patriot missile defense'
            FROM {C}
            USING HYBRID DENSE dense SPARSE sparse FUSION RRF
            WITH PAYLOAD true LIMIT 3
            """,
            tenant=tenant,
        )
        if isinstance(hits, dict):
            hits = hits.get("result", hits)
            if isinstance(hits, dict):
                hits = hits.get("points") or hits.get("hits") or []
        found = {(h.get("payload") or {}).get("tenant_id", "?") for h in hits}
        print(f"  [{tenant:>9s}] → tenants in results: {found}")
    except Exception as e:
        print(f"  [{tenant}] ERROR: {e}")

# ── LLM answer ──
print("\n═══ 15. LLM ANSWER — RTX missile contracts ═══")
try:
    hits = run(
        f"""
        QUERY TEXT 'Raytheon contract awards programs'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WITH PAYLOAD true LIMIT 3
        """,
        tenant="rtx",
    )
    print(f"  {llm('What missile defense programs did Raytheon win?', hits)[:400]}")
except Exception as e:
    print(f"  skipped: {e}")

print(f"\n✅ Showcase complete — hybrid, CTE, formula, ACORN, COUNT exact, isolation.")
