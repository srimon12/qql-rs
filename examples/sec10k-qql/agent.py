"""
Agentic QQL — LLM picks retrieval strategies; host always injects isolation.

Three tools, each a different QQL strategy:
  strategy_hybrid       — dense + sparse fusion (USING HYBRID shorthand)
  strategy_multistage   — CTE + PREFETCH + FUSION
  strategy_formula      — score boosting with FORMULA

Isolation is never left to the model:
  inject_filter(stmt, "tenant_id", "=", tenant)
  inject_shard_key(stmt, tenant)
"""

from __future__ import annotations

import json
import os
import sys

import requests

sys.path.insert(0, os.environ.get("QQL_LIB", os.path.join(os.path.dirname(__file__), "../../crates/pyqql")))
import pyqql  # noqa: E402
import config  # noqa: E402

C = config.COLLECTION
LM = config.LM_STUDIO.rstrip("/")


def run_qql(qql: str, tenant: str, year: int | None = None):
    """Execute QQL with tenant + optional year isolation (host-side only)."""
    tenant = tenant.lower().strip()
    stmt = pyqql.parse(qql)[0]
    pyqql.inject_filter(stmt, "tenant_id", "=", tenant)
    pyqql.inject_shard_key(stmt, tenant)
    if year is not None:
        pyqql.inject_filter(stmt, "fiscal_year", "=", year)

    embed_url = LM if LM.endswith("/embeddings") else f"{LM}/v1/embeddings"
    e = pyqql.HttpEmbedder(embed_url, config.EMBED_MODEL, config.EMBED_DIM)
    client = pyqql.Client(config.QDRANT_URL, embedder=e)
    resp = client.execute(stmt)
    return resp["results"][0].get("data", [])


def format_hits(hits):
    if isinstance(hits, dict):
        hits = hits.get("result", hits)
        if isinstance(hits, dict):
            hits = hits.get("points") or hits.get("hits") or []
    return [
        {
            "tenant": (h.get("payload") or {}).get("tenant_id", "?"),
            "year": (h.get("payload") or {}).get("fiscal_year", "?"),
            "score": h.get("score", 0),
            "text": (h.get("text") or (h.get("payload") or {}).get("text") or "")[:500],
        }
        for h in hits
    ]


def strategy_hybrid(tenant, query, year=None, limit=5):
    """Hybrid: USING HYBRID shorthand (same plan as QUERY HYBRID)."""
    q = query.replace("'", "\\'")
    qql = f"""
        QUERY TEXT '{q}'
        FROM {C}
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        WITH PAYLOAD true LIMIT {limit}
    """
    return format_hits(run_qql(qql, tenant, year))


def strategy_multistage(tenant, query, year=None, limit=5):
    """CTE dense candidates → RRF fusion."""
    q = query.replace("'", "\\'")
    qql = f"""
        WITH dense_candidates AS (
          QUERY TEXT '{q}' FROM {C} USING dense LIMIT 100
        )
        QUERY FUSION RRF FROM {C}
        PREFETCH (dense_candidates)
        WITH PAYLOAD true LIMIT {limit}
    """
    return format_hits(run_qql(qql, tenant, year))


def strategy_formula(tenant, query, year=None, limit=5):
    """Formula scoring over dense candidates."""
    q = query.replace("'", "\\'")
    qql = f"""
        WITH candidates AS (
          QUERY TEXT '{q}' FROM {C} USING dense LIMIT 30
        )
        QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0)
        FROM {C}
        PREFETCH (candidates)
        WITH PAYLOAD true LIMIT {limit}
    """
    return format_hits(run_qql(qql, tenant, year))


TOOLS = [
    {
        "type": "function",
        "name": "strategy_hybrid",
        "description": (
            "Hybrid search (dense + sparse RRF). Best for general questions, "
            "risk factors, business descriptions.\n"
            "QUERY TEXT '…' FROM … USING HYBRID DENSE dense SPARSE sparse FUSION RRF"
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS},
                "query": {"type": "string", "description": "Specific keyword query"},
                "year": {"type": "integer", "description": "Optional fiscal year"},
            },
            "required": ["tenant", "query"],
        },
    },
    {
        "type": "function",
        "name": "strategy_multistage",
        "description": (
            "Multi-stage: dense 100 candidates then RRF. Best for complex "
            "questions and specific fact-finding.\n"
            "WITH dense_candidates AS (…) QUERY FUSION RRF PREFETCH (…)"
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS},
                "query": {"type": "string"},
                "year": {"type": "integer"},
            },
            "required": ["tenant", "query"],
        },
    },
    {
        "type": "function",
        "name": "strategy_formula",
        "description": (
            "Formula score boost (score * 2.0). Best for ranking / top matches."
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS},
                "query": {"type": "string"},
                "year": {"type": "integer"},
            },
            "required": ["tenant", "query"],
        },
    },
]

STRATEGIES = {
    "strategy_hybrid": strategy_hybrid,
    "strategy_multistage": strategy_multistage,
    "strategy_formula": strategy_formula,
}

SYSTEM_PROMPT = (
    "You are a financial analyst. Use retrieval tools to search SEC 10-K filings.\n\n"
    "WHEN TO USE EACH STRATEGY:\n"
    "- strategy_hybrid: general questions, risks, business descriptions\n"
    "- strategy_multistage: complex questions, comparisons, finding specific facts\n"
    "- strategy_formula: ranking, financial data, finding top matches\n\n"
    "For comparison questions, call the SAME strategy for BOTH companies.\n"
    "Craft query strings with SPECIFIC keywords: company terms, product names, "
    "financial concepts, and years."
)


def call_llm(user_input: str):
    r = requests.post(
        f"{LM}/v1/responses",
        json={
            "model": config.LLM_MODEL,
            "input": user_input,
            "instructions": SYSTEM_PROMPT,
            "tools": TOOLS,
            "tool_choice": "auto",
        },
        timeout=180,
    )
    return r.json()


def parse_output(raw):
    if isinstance(raw, list):
        return raw
    if isinstance(raw, str):
        try:
            return json.loads(raw)
        except Exception:
            pass
        try:
            import ast

            return ast.literal_eval(raw)
        except Exception:
            return []
    return []


def synthesize(question: str, all_hits: list) -> str:
    ctx = "\n\n".join(
        f"[{h['tenant']} FY{h['year']} s={h['score']:.3f}] {h['text'][:400]}"
        for h in all_hits[:12]
    )
    r = requests.post(
        f"{LM}/v1/chat/completions",
        json={
            "model": config.LLM_MODEL,
            "messages": [
                {
                    "role": "user",
                    "content": (
                        f"Question: {question}\n\nExcerpts from SEC 10-K filings:\n{ctx}\n\n"
                        f"Based ONLY on these excerpts, answer with specific facts and figures. "
                        f"Cite company and year. If not enough info, say so."
                    ),
                }
            ],
        },
        timeout=180,
    )
    return r.json()["choices"][0]["message"]["content"]


def run_agent(user_input: str) -> None:
    print(f"\n{'=' * 60}")
    print(f"QUESTION: {user_input}")
    print(f"{'=' * 60}")

    data = call_llm(user_input)
    output = parse_output(data.get("output", []))
    calls = [o for o in output if o.get("type") == "function_call"]

    if not calls:
        print("  (no tool calls)")
        return

    all_hits = []
    for tc in calls:
        name = tc["name"]
        args = json.loads(tc.get("arguments", "{}"))
        tenant = args.get("tenant", "?").lower().strip()
        query = args.get("query", "")
        year = args.get("year")
        fn = STRATEGIES.get(name)
        if not fn:
            print(f"  Unknown strategy: {name}")
            continue

        print(f"\n🔧 {name}({tenant}, '{query[:60]}', year={year})")
        print(f"   isolation: inject_filter(tenant_id) + inject_shard_key('{tenant}')")
        hits = fn(tenant, query, year)
        all_hits.extend(hits)
        print(f"   → {len(hits)} hits")
        for h in hits[:2]:
            print(f"      s={h['score']:.4f} {h['tenant']} FY{h['year']} | {h['text'][:120]}...")

    if not all_hits:
        print("\nNo results.")
        return

    print(f"\n{'─' * 60}")
    answer = synthesize(user_input, all_hits)
    print(f"\n📊 ANSWER:\n{answer}")


if __name__ == "__main__":
    if len(sys.argv) > 1:
        run_agent(" ".join(sys.argv[1:]))
    else:
        for q in [
            "What are Honeywell's cybersecurity risks?",
            "Compare GE and RTX aerospace and defense businesses in 2024",
            "What were 3M's largest financial figures in 2024?",
        ]:
            try:
                run_agent(q)
            except Exception as e:
                print(f"ERROR: {e}")
            print()
