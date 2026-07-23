"""
Agentic QQL — LLM picks retrieval strategies, we execute full QQL patterns.

Three tools, each a different QQL strategy:
  strategy_hybrid       — dense + sparse fusion (good for general questions)
  strategy_multistage   — CTE + PREFETCH + FUSION (good for complex questions)
  strategy_formula      — score boosting with FORMULA (good for ranking)

The LLM decides which strategy and what to search for.
"""

import sys, os, json, requests, re
sys.path.insert(0, os.environ.get("QQL_LIB", "../../target/release"))
import pyqql
import config

C = config.COLLECTION
LM = "http://127.0.0.1:1234"


def run_qql(qql, tenant, year=None):
    """Execute QQL with tenant + optional year isolation."""
    tenant = tenant.lower().strip()  # Qdrant shard keys are lowercase
    stmt = pyqql.parse(qql)
    pyqql.inject_filter(stmt, "tenant_id", "=", tenant)
    if year:
        pyqql.inject_filter(stmt, "fiscal_year", "=", year)
    e = pyqql.HttpEmbedder(f"{LM}/v1/embeddings", config.EMBED_MODEL, config.EMBED_DIM)
    client = pyqql.Client(config.QDRANT_URL, embedder=e)
    resp = client.execute(stmt)
    return resp.get("data", [])


def format_hits(hits):
    """Clean results for LLM consumption."""
    return [{
        "tenant": (h.get("payload") or {}).get("tenant_id", "?"),
        "year": (h.get("payload") or {}).get("fiscal_year", "?"),
        "score": h.get("score", 0),
        "text": (h.get("text") or "")[:500],
    } for h in hits]


# ═══════════════════════════════════════════════════════════════
# THREE RETRIEVAL STRATEGIES — each with real QQL power
# ═══════════════════════════════════════════════════════════════

def strategy_hybrid(tenant, query, year=None, limit=5):
    """Simple hybrid: dense + sparse fused with RRF."""
    qql = f"QUERY HYBRID TEXT '{query}' DENSE dense SPARSE sparse FUSION RRF FROM {C} SHARD '{tenant}' WITH PAYLOAD true LIMIT {limit}"
    return format_hits(run_qql(qql, tenant, year))

def strategy_multistage(tenant, query, year=None, limit=5):
    """CTE + PREFETCH + FUSION: dense candidates → fuse with sparse → rerank."""
    qql = f"""
        WITH dense_candidates AS (QUERY TEXT '{query}' FROM {C} USING dense LIMIT 100)
        QUERY FUSION RRF FROM {C}
        PREFETCH (dense_candidates)
        SHARD '{tenant}'
        WITH PAYLOAD true LIMIT {limit}
    """
    return format_hits(run_qql(qql, tenant, year))

def strategy_formula(tenant, query, year=None, limit=5):
    """Formula scoring: boost results using custom score formula."""
    qql = f"""
        WITH candidates AS (QUERY TEXT '{query}' FROM {C} USING dense LIMIT 30)
        QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0)
        FROM {C}
        PREFETCH (candidates)
        SHARD '{tenant}'
        WITH PAYLOAD true LIMIT {limit}
    """
    return format_hits(run_qql(qql, tenant, year))


# ═══════════════════════════════════════════════════════════════
# TOOL DEFINITIONS — LLM sees these
# ═══════════════════════════════════════════════════════════════

TOOLS = [
    {
        "type": "function",
        "name": "strategy_hybrid",
        "description": (
            "Hybrid search (dense + sparse RRF fusion). Best for: general "
            "questions, risk factors, business descriptions.\n"
            "QUERY HYBRID TEXT '...' DENSE dense SPARSE sparse FUSION RRF FROM ... SHARD ... WITH PAYLOAD true LIMIT 5"
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS, "description": "Company to search"},
                "query": {"type": "string", "description": "Specific keyword query — combine company terms, concepts, and year"},
                "year": {"type": "integer", "description": "Optional fiscal year filter"},
            },
            "required": ["tenant", "query"]
        }
    },
    {
        "type": "function",
        "name": "strategy_multistage",
        "description": (
            "Multi-stage retrieval: dense search first (100 candidates), "
            "then fuse with RRF. Best for: complex questions, comparisons, "
            "finding specific facts in large datasets.\n"
            "WITH dense_candidates AS (QUERY TEXT '...' FROM ... USING dense LIMIT 100) "
            "QUERY FUSION RRF FROM ... PREFETCH (dense_candidates) ..."
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS, "description": "Company to search"},
                "query": {"type": "string", "description": "Specific keyword query"},
                "year": {"type": "integer", "description": "Optional fiscal year filter"},
            },
            "required": ["tenant", "query"]
        }
    },
    {
        "type": "function",
        "name": "strategy_formula",
        "description": (
            "Formula-based scoring: boosts relevant results with score * 2.0. "
            "Best for: ranking, finding top matches, financial data retrieval.\n"
            "WITH candidates AS (QUERY TEXT '...' FROM ... USING dense LIMIT 30) "
            "QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0) FROM ... PREFETCH (candidates) ..."
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "tenant": {"type": "string", "enum": config.TENANTS, "description": "Company to search"},
                "query": {"type": "string", "description": "Specific keyword query for financial/specific data"},
                "year": {"type": "integer", "description": "Optional fiscal year filter"},
            },
            "required": ["tenant", "query"]
        }
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
    "financial concepts, and years. Example: 'supply chain shortages raw materials pricing volatility'"
)


def call_llm(user_input):
    r = requests.post(f"{LM}/v1/responses", json={
        "model": config.LLM_MODEL,
        "input": user_input,
        "instructions": SYSTEM_PROMPT,
        "tools": TOOLS,
        "tool_choice": "auto",
    }, timeout=180)
    return r.json()


def parse_output(raw):
    if isinstance(raw, list): return raw
    if isinstance(raw, str):
        try: return json.loads(raw)
        except: pass
        try: import ast; return ast.literal_eval(raw)
        except: return []
    return []


def synthesize(question, all_hits):
    """LLM synthesizes final answer from retrieved chunks."""
    ctx = "\n\n".join(
        f"[{h['tenant']} FY{h['year']} s={h['score']:.3f}] {h['text'][:400]}"
        for h in all_hits[:12]
    )
    r = requests.post(f"{LM}/v1/chat/completions", json={
        "model": config.LLM_MODEL,
        "messages": [{"role": "user", "content": (
            f"Question: {question}\n\nExcerpts from SEC 10-K filings:\n{ctx}\n\n"
            f"Based ONLY on these excerpts, answer with specific facts and figures. "
            f"Cite company and year. If not enough info, say so."
        )}],
    }, timeout=180)
    return r.json()["choices"][0]["message"]["content"]


def run_agent(user_input):
    print(f"\n{'='*60}")
    print(f"QUESTION: {user_input}")
    print(f"{'='*60}")

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

        # Show the actual QQL being executed
        print(f"\n🔧 {name}({tenant}, '{query[:60]}', year={year})")
        hits = fn(tenant, query, year)
        all_hits.extend(hits)
        print(f"   → {len(hits)} hits")
        for h in hits[:2]:
            print(f"      s={h['score']:.4f} {h['tenant']} FY{h['year']} | {h['text'][:120]}...")

    if not all_hits:
        print("\nNo results.")
        return

    print(f"\n{'─'*60}")
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
