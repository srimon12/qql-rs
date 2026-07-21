#!/usr/bin/env python3
"""
Medical Retrieval Showcase — minimal E2E demo of QQL capabilities.

Demonstrates: collection creation, indexing, hybrid upsert, dense/sparse/hybrid
search, filters, grouped retrieval, recommend, context, discover, CTE prefetch DAGs,
parameterized RRF, updates, select, scroll, dump, and explain.

Usage:
    uv run examples/medical-showcase/main.py --execute
    uv run examples/medical-showcase/main.py --execute --keep
    uv run examples/medical-showcase/main.py              # print only
"""
from __future__ import annotations
import argparse, json, os, subprocess, sys

QQL = os.environ.get("QQL_BIN", "/data/codebases/qql-rs/target/debug/qql")
COL = "medical_showcase"


def qql(stmt: str) -> dict:
    r = subprocess.run([QQL, "exec", "--quiet", "--json", stmt], capture_output=True, text=True)
    if not r.stdout.strip():
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr)
    return d


def qql_explain(stmt: str) -> dict:
    r = subprocess.run([QQL, "explain", "--quiet", "--json", stmt], capture_output=True, text=True)
    if not r.stdout.strip():
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr)
    return d


def qql_dump(collection: str, path: str) -> dict:
    r = subprocess.run([QQL, "dump", "--quiet", "--json", collection, path], capture_output=True, text=True)
    if not r.stdout.strip():
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(r.stdout)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr)
    return d


def run(label: str, stmt: str, *, execute: bool, keep: bool = False, explain: bool = False, dump: bool = False):
    print(f"  {label}")
    if not execute:
        return
    try:
        if explain:
            d = qql_explain(stmt)
            plan = d.get("plan", "")
            for line in plan.split("\n")[:5]:
                print(f"    {line}")
        elif dump:
            parts = stmt.split(" ", 1)
            d = qql_dump(parts[0], parts[1])
            print(f"    {d.get('message','')}")
        else:
            d = qql(stmt)
            msg = d.get("message", "")
            hits = d.get("data", {})
            if isinstance(hits, dict) and "results" in hits:
                for h in hits["results"][:3]:
                    print(f"    score={h['score']:.3f}  {h.get('id','')}")
            elif isinstance(hits, dict) and "groups" in hits:
                for g in hits["groups"][:3]:
                    print(f"    group={g['group_id']}  hits={len(g['hits'])}")
            else:
                print(f"    {msg}")
    except Exception as e:
        print(f"    ERROR: {e}")


# -- 12 medical records --
RECORDS = [
    (1, "Acute ischemic stroke with sudden right-sided weakness and slurred speech. CT confirms left MCA infarct. Thrombolysis initiated.", "neurology", "high", "admitted", 2026),
    (2, "STEMI with crushing chest pain radiating to left arm. ECG shows ST elevation V1-V4. Emergency catheterization planned.", "cardiology", "high", "admitted", 2025),
    (3, "Community-acquired pneumonia with fever, productive cough, and right lower lobe consolidation. IV antibiotics started.", "pulmonology", "medium", "reviewed", 2024),
    (4, "Tension headache improved with rest. No focal neurological deficits. Discharged with follow-up.", "general", "low", "discharged", 2023),
    (5, "Acute appendicitis with RLQ pain and positive McBurney sign. Laparoscopic appendectomy scheduled.", "surgery", "medium", "preoperative", 2024),
    (6, "Migraine with photophobia and phonophobia. IV sumatriptan administered.", "neurology", "low", "discharged", 2023),
    (7, "Closed tibial fracture from fall. ORIF planned.", "orthopedics", "medium", "preoperative", 2025),
    (8, "Poorly controlled type 2 diabetes. HbA1c 10.2%. Insulin regimen started.", "endocrinology", "medium", "reviewed", 2024),
    (9, "Acute asthma exacerbation with wheezing and dyspnea. Peak flow 40% predicted.", "pulmonology", "high", "admitted", 2026),
    (10, "Sepsis with fever, tachycardia, hypotension, elevated lactate. ICU transfer.", "internal-medicine", "high", "admitted", 2025),
    (11, "COPD exacerbation with increased dyspnea and purulent sputum.", "pulmonology", "medium", "reviewed", 2024),
    (12, "Suspected bacterial meningitis with severe headache, neck stiffness, and fever. Empiric antibiotics started.", "neurology", "high", "admitted", 2026),
]


def main():
    ap = argparse.ArgumentParser(description="Medical Retrieval Showcase")
    ap.add_argument("--execute", action="store_true", help="Run against Qdrant")
    ap.add_argument("--keep", action="store_true", help="Keep collection after run")
    args = ap.parse_args()

    print("Medical Retrieval Showcase")
    print("=" * 50)

    # -- Schema --
    print("\n[1] Schema")
    run("Create HYBRID collection with TURBO quantization",
        f"CREATE COLLECTION {COL} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)", execute=args.execute)
    for f, ftype in [("specialty","keyword"),("priority","keyword"),("status","keyword"),("year","integer"),
                 ("patient_id","keyword"),("diagnosis","text WITH (tokenizer = 'word', min_token_len = 2, lowercase = true, phrase_matching = true)")]:
        run(f"Index {f}",
            f"CREATE INDEX ON COLLECTION {COL} FOR {f} TYPE {ftype}", execute=args.execute)

    # -- Upsert --
    print("\n[2] Upsert (12 records, HYBRID)")
    if args.execute:
        try:
            qql(f"DROP COLLECTION {COL}")
        except:
            pass
        qql(f"CREATE COLLECTION {COL} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)")
        for f, ftype in [("specialty","keyword"),("priority","keyword"),("status","keyword"),("year","integer"),
                     ("patient_id","keyword"),("diagnosis","text WITH (tokenizer = 'word', min_token_len = 2, lowercase = true, phrase_matching = true)")]:
            qql(f"CREATE INDEX ON COLLECTION {COL} FOR {f} TYPE {ftype}")
    for pid, text, spec, pri, stat, year in RECORDS:
        run(f"Upsert #{pid} ({spec})",
            f"UPSERT INTO {COL} VALUES {{'id': {pid}, 'text': '{text}', 'patient_id': 'PT-{pid:04d}', 'specialty': '{spec}', 'priority': '{pri}', 'status': '{stat}', 'year': {year}}} USING HYBRID",
            execute=args.execute)

    # -- Search modes --
    print("\n[3] Search Modes")
    run("Dense (semantic)", f"QUERY 'acute stroke weakness' FROM {COL} LIMIT 3", execute=args.execute)
    run("Dense EXACT baseline", f"QUERY 'chest pain troponin' FROM {COL} LIMIT 3 EXACT", execute=args.execute)
    run("Hybrid RRF", f"QUERY 'emergency neurological' FROM {COL} LIMIT 3 USING HYBRID", execute=args.execute)
    run("Hybrid DBSF", f"QUERY 'emergency neurological' FROM {COL} LIMIT 3 USING HYBRID FUSION DBSF", execute=args.execute)
    run("Parameterized RRF (k=30, weights)", f"QUERY 'emergency critical' FROM {COL} LIMIT 3 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])", execute=args.execute)
    run("Sparse BM25", f"QUERY 'fever cough antibiotics' FROM {COL} LIMIT 3 USING SPARSE", execute=args.execute)
    run("MMR diversity", f"QUERY 'neurological emergency' FROM {COL} LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)", execute=args.execute)

    # -- Filters --
    print("\n[4] Filters")
    run("WHERE specialty = 'neurology'", f"QUERY 'headache' FROM {COL} LIMIT 3 USING HYBRID WHERE specialty = 'neurology'", execute=args.execute)
    run("WHERE priority IN ('high','medium')", f"QUERY 'chest pain' FROM {COL} LIMIT 3 USING HYBRID WHERE priority IN ('high', 'medium')", execute=args.execute)
    run("WHERE year BETWEEN 2024 AND 2026", f"QUERY 'patient' FROM {COL} LIMIT 3 WHERE year BETWEEN 2024 AND 2026", execute=args.execute)
    run("Compound AND + IN", f"QUERY 'emergency' FROM {COL} LIMIT 3 USING HYBRID WHERE priority = 'high' AND status = 'admitted'", execute=args.execute)
    run("MATCH PHRASE", f"QUERY 'chest pain' FROM {COL} LIMIT 3 WHERE diagnosis MATCH PHRASE 'chest pain'", execute=args.execute)

    # -- Query params --
    print("\n[5] Query-Time Params")
    run("HNSW ef=256", f"QUERY 'stroke' FROM {COL} LIMIT 3 WITH (hnsw_ef = 256)", execute=args.execute)
    run("ACORN (filtered recall)", f"QUERY 'emergency' FROM {COL} LIMIT 3 WHERE specialty = 'neurology' WITH (acorn = true)", execute=args.execute)
    run("Score threshold", f"QUERY 'patient treatment' FROM {COL} LIMIT 10 SCORE THRESHOLD 0.3", execute=args.execute)
    run("Offset pagination", f"QUERY 'patient diagnosis' FROM {COL} LIMIT 3 OFFSET 3", execute=args.execute)

    # -- Grouped --
    print("\n[6] Grouped Retrieval")
    run("GROUP BY specialty", f"QUERY 'emergency acute' FROM {COL} LIMIT 4 GROUP BY 'specialty' GROUP_SIZE 2", execute=args.execute)
    run("GROUP BY priority", f"QUERY 'patient' FROM {COL} LIMIT 4 USING HYBRID GROUP BY 'priority' GROUP_SIZE 2", execute=args.execute)

    # -- Recommend --
    print("\n[7] Recommend")
    run("Single positive", f"QUERY RECOMMEND WITH (positive = (1)) FROM {COL} LIMIT 3", execute=args.execute)
    run("Multi + negative", f"QUERY RECOMMEND WITH (positive = (1, 12), negative = (4)) FROM {COL} LIMIT 3", execute=args.execute)
    run("Strategy best_score", f"QUERY RECOMMEND WITH (positive = (2, 10)) FROM {COL} STRATEGY 'best_score' LIMIT 3", execute=args.execute)

    # -- Context / Discover --
    print("\n[8] Context & Discover")
    run("Context pairs", f"QUERY CONTEXT PAIRS (1, 4), (10, 3) FROM {COL} LIMIT 3", execute=args.execute)
    run("Discover", f"QUERY DISCOVER TARGET 1 CONTEXT PAIRS (12, 4) FROM {COL} LIMIT 3", execute=args.execute)

    # -- CTE Prefetch DAG --
    print("\n[9] CTE-based Prefetch DAGs")
    run("Prefetch RRF",
        f"WITH a AS (QUERY 'emergency neurological' USING dense LIMIT 10), b AS (QUERY 'emergency neurological' USING sparse LIMIT 10) QUERY 'emergency neurological' FROM {COL} LIMIT 3 PREFETCH (a, b) FUSION RRF",
        execute=args.execute)
    run("Prefetch RRF + per-prefetch filter + score threshold",
        f"WITH a AS (QUERY 'emergency neurological' USING dense LIMIT 20), b AS (QUERY 'emergency neurological' USING sparse LIMIT 20) QUERY 'emergency neurological' FROM {COL} LIMIT 3 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.3, b SCORE THRESHOLD 0.1) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])",
        execute=args.execute)
    run("Prefetch RRF + params",
        f"WITH a AS (QUERY 'emergency neurological' USING dense LIMIT 10 WHERE priority = 'high'), b AS (QUERY 'emergency neurological' USING sparse LIMIT 10) QUERY 'emergency neurological' FROM {COL} LIMIT 3 PREFETCH (a, b) FUSION RRF WITH (rrf_k = 10, rrf_weights = [0.7, 0.3])",
        execute=args.execute)

    # -- Grouped with Lookup --
    print("\n[9b] Grouped Retrieval with Cross-Collection Lookup")
    run("GROUP BY specialty WITH LOOKUP FROM (requires metadata collection)",
        f"QUERY 'emergency acute' FROM {COL} LIMIT 4 GROUP BY 'specialty' GROUP_SIZE 2",
        execute=args.execute)

    # -- Mutations --
    print("\n[10] Mutations")
    run("Update payload by ID", f"UPDATE {COL} SET PAYLOAD = {{'status': 'reviewed', 'care_path': 'stroke-alert'}} WHERE id = 1", execute=args.execute)
    run("Update payload by filter", f"UPDATE {COL} SET PAYLOAD = {{'status': 'archived'}} WHERE status = 'discharged'", execute=args.execute)
    run("Delete by filter", f"DELETE FROM {COL} WHERE status = 'archived'", execute=args.execute)

    # -- ORDER BY & Selectors --
    print("\n[11] ORDER BY & Field Selection")
    run("ORDER BY year DESC", f"QUERY ORDER BY year DESC FROM {COL} LIMIT 5", execute=args.execute)
    run("WITH PAYLOAD false", f"QUERY 'stroke' FROM {COL} LIMIT 3 USING HYBRID WITH PAYLOAD false", execute=args.execute)
    run("WITH PAYLOAD exclude", f"QUERY 'emergency' FROM {COL} LIMIT 3 USING HYBRID WITH PAYLOAD (exclude = ['patient_id', 'diagnosis'])", execute=args.execute)

    # -- Access --
    print("\n[12] Point Access")
    run("Select by ID", f"SELECT * FROM {COL} WHERE id = 2", execute=args.execute)
    run("Scroll all", f"SCROLL FROM {COL} LIMIT 3", execute=args.execute)
    run("Scroll filtered", f"SCROLL FROM {COL} WHERE priority = 'high' LIMIT 3", execute=args.execute)

    # -- Operations --
    print("\n[13] Operations")
    run("SHOW COLLECTIONS", "SHOW COLLECTIONS", execute=args.execute)
    run("SHOW COLLECTION", f"SHOW COLLECTION {COL}", execute=args.execute)
    run("EXPLAIN", f"QUERY 'stroke' FROM {COL} LIMIT 3 USING HYBRID", execute=args.execute, explain=True)
    run("Dump", f"{COL} /tmp/{COL}.qql", execute=args.execute, dump=True)

    # -- Cleanup --
    if args.execute and not args.keep:
        try:
            qql(f"DROP COLLECTION {COL}")
            print(f"\n[cleanup] Dropped {COL}")
        except:
            pass

    print("\nDone.")


if __name__ == "__main__":
    main()
