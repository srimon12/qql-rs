#!/usr/bin/env python3
"""
Medical Retrieval Showcase — E2E demo of QQL capabilities.

Demonstrates: collection creation, indexing, hybrid upsert, dense/sparse/hybrid
(USING HYBRID), filters, ACORN, request timeout/consistency, grouped retrieval,
recommend, context, discover, CTE prefetch DAGs, parameterized RRF, mutations,
DELETE PAYLOAD, SCROLL WITH VECTOR, COUNT exact, explain.

Usage:
    QQL_BIN=./target/release/qql python examples/medical-showcase/main.py --execute
    QQL_BIN=./target/release/qql python examples/medical-showcase/main.py --execute --keep
    python examples/medical-showcase/main.py              # print only
"""
from __future__ import annotations
import argparse, json, os, subprocess, sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
QQL = os.environ.get("QQL_BIN", str(_ROOT / "target" / "release" / "qql"))
COL = "medical_showcase"


def qql_exec(stmt: str) -> dict:
    # Prefer JSON without --quiet so successful DDL always emits a report body.
    r = subprocess.run([QQL, "exec", "--json", stmt], capture_output=True, text=True)
    out = (r.stdout or "").strip()
    if not out:
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(out)
    if not d.get("ok"):
        # Multi-result report: ok=false when any statement failed
        err = d.get("error") or r.stderr
        if not err and d.get("results"):
            err = next((x.get("message") for x in d["results"] if not x.get("ok")), None)
        raise RuntimeError(err or "execution failed")
    return d


def qql_explain(stmt: str) -> dict:
    r = subprocess.run([QQL, "explain", "--json", stmt], capture_output=True, text=True)
    out = (r.stdout or "").strip()
    if not out:
        raise RuntimeError(r.stderr or f"exit {r.returncode}")
    d = json.loads(out)
    if not d.get("ok"):
        raise RuntimeError(d.get("error") or r.stderr)
    return d


def run(label: str, stmt: str, *, execute: bool, explain: bool = False):
    print(f"  {label}")
    if not execute:
        return
    try:
        if explain:
            d = qql_explain(stmt)
            plan = d.get("plan", "")
            for line in str(plan).split("\n")[:5]:
                print(f"    {line}")
            return
        d = qql_exec(stmt)
        # Normalize ExecutionReport → first result
        if "results" in d:
            res = d["results"][0] if d["results"] else {}
        else:
            res = d
        msg = res.get("message", d.get("message", "ok"))
        data = res.get("data")
        if isinstance(data, dict):
            data = data.get("result", data)
            if isinstance(data, dict):
                if "count" in data:
                    print(f"    count={data['count']}")
                    return
                data = data.get("points") or data.get("hits") or data.get("groups") or data
        if isinstance(data, list) and data:
            if isinstance(data[0], dict) and "score" in data[0]:
                for h in data[:3]:
                    print(f"    score={h['score']:.3f}  id={h.get('id', '')}")
                return
            if isinstance(data[0], dict) and "hits" in data[0]:
                for g in data[:3]:
                    print(f"    group={g.get('id', '?')} hits={len(g.get('hits', []))}")
                return
        print(f"    {msg or 'ok'}")
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

    # Quant rescore defaults for turbo collections
    QRESCORE = "PARAMS (hnsw_ef = 128, quantization = {ignore: false, rescore: true, oversampling: 2.0})"

    # -- Schema --
    print("\n[1] Schema")
    if args.execute:
        try:
            qql_exec(f"DROP COLLECTION {COL}")
            print("  Drop existing collection")
        except Exception:
            print("  Drop (none)")
    run("Create HYBRID + turbo/2",
        f"CREATE COLLECTION {COL} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)",
        execute=args.execute)
    for name, ftype in [
        ("specialty", "keyword"), ("priority", "keyword"), ("status", "keyword"),
        ("year", "integer"), ("patient_id", "keyword"),
    ]:
        run(f"  Index {name}",
            f"CREATE INDEX ON COLLECTION {COL} FOR {name} TYPE {ftype}",
            execute=args.execute)

    # -- Upsert --
    print("\n[2] Upsert (12 records)")
    for pid, text, spec, pri, stat, year in RECORDS:
        run(f"  Upsert #{pid} ({spec})",
            f"UPSERT INTO {COL} VALUES {{'id': {pid}, 'text': '{text}', 'patient_id': 'PT-{pid:04d}', 'specialty': '{spec}', 'priority': '{pri}', 'status': '{stat}', 'year': {year}}}",
            execute=args.execute)

    # -- Search Modes --
    print("\n[3] Search Modes")
    run("Dense (semantic) + rescore",
        f"QUERY TEXT 'acute stroke weakness' FROM {COL} USING dense {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("Dense exact (PARAMS exact=true)",
        f"QUERY TEXT 'chest pain troponin' FROM {COL} USING dense PARAMS (exact = true) LIMIT 3",
        execute=args.execute)
    run("Hybrid RRF + rescore",
        f"QUERY HYBRID TEXT 'emergency neurological' DENSE dense SPARSE sparse FUSION RRF FROM {COL} {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("Hybrid DBSF",
        f"QUERY HYBRID TEXT 'emergency neurological' DENSE dense SPARSE sparse FUSION DBSF FROM {COL} {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("Sparse (keyword)",
        f"QUERY TEXT 'fever cough antibiotics' FROM {COL} USING sparse LIMIT 3",
        execute=args.execute)
    run("MMR diversity",
        f"QUERY MMR TEXT 'neurological emergency' DIVERSITY 0.5 CANDIDATES 20 FROM {COL} USING dense {QRESCORE} LIMIT 5",
        execute=args.execute)

    # -- Filters --
    print("\n[4] Filters")
    run("WHERE specialty = 'neurology'",
        f"QUERY TEXT 'headache' FROM {COL} USING dense WHERE specialty = 'neurology' {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("WHERE priority IN ('high','medium')",
        f"QUERY TEXT 'chest pain' FROM {COL} USING dense WHERE priority IN ('high', 'medium') {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("Compound AND + IN",
        f"QUERY TEXT 'emergency' FROM {COL} USING dense WHERE priority = 'high' AND status = 'admitted' {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("MATCH PHRASE",
        f"QUERY TEXT 'chest pain' FROM {COL} USING dense WHERE text MATCH PHRASE 'chest pain' {QRESCORE} LIMIT 3",
        execute=args.execute)

    # -- Query-Time Params --
    print("\n[5] Query-Time Params")
    run("HNSW ef=256 + rescore",
        f"QUERY TEXT 'stroke' FROM {COL} USING dense PARAMS (hnsw_ef = 256, quantization = {{rescore: true, oversampling: 2.0}}) LIMIT 3",
        execute=args.execute)
    run("ACORN (filtered recall)",
        f"QUERY TEXT 'emergency' FROM {COL} USING dense WHERE specialty = 'neurology' PARAMS (acorn = true, max_selectivity = 0.5, quantization = {{rescore: true}}) LIMIT 3",
        execute=args.execute)
    run("Hybrid shorthand (USING HYBRID)",
        f"QUERY TEXT 'emergency neurological' FROM {COL} USING HYBRID DENSE dense SPARSE sparse FUSION RRF {QRESCORE} LIMIT 3",
        execute=args.execute)
    run("Request timeout + consistency",
        f"QUERY TEXT 'stroke' FROM {COL} USING dense PARAMS (timeout = 15, consistency = majority, quantization = {{rescore: true}}) LIMIT 3",
        execute=args.execute)
    run("Score threshold",
        f"QUERY TEXT 'patient treatment' FROM {COL} USING dense {QRESCORE} SCORE THRESHOLD 0.3 LIMIT 10",
        execute=args.execute)
    run("Offset pagination",
        f"QUERY TEXT 'patient diagnosis' FROM {COL} USING dense {QRESCORE} LIMIT 3 OFFSET 3",
        execute=args.execute)

    # -- Grouped Retrieval --
    print("\n[6] Grouped Retrieval")
    run("GROUP BY specialty",
        f"QUERY TEXT 'emergency acute' FROM {COL} USING dense {QRESCORE} GROUP BY specialty SIZE 2 LIMIT 4",
        execute=args.execute)
    run("GROUP BY priority",
        f"QUERY TEXT 'patient' FROM {COL} USING dense {QRESCORE} GROUP BY priority SIZE 2 LIMIT 4",
        execute=args.execute)

    # -- Recommend --
    print("\n[7] Recommend")
    run("Single positive",
        f"QUERY RECOMMEND POSITIVE (1) STRATEGY average_vector FROM {COL} USING dense LIMIT 3",
        execute=args.execute)
    run("Multi positive + negative",
        f"QUERY RECOMMEND POSITIVE (1, 12) NEGATIVE (4) STRATEGY average_vector FROM {COL} USING dense LIMIT 3",
        execute=args.execute)
    run("Strategy best_score",
        f"QUERY RECOMMEND POSITIVE (2, 10) STRATEGY best_score FROM {COL} USING dense LIMIT 3",
        execute=args.execute)

    # -- Context & Discover --
    print("\n[8] Context & Discover")
    run("Context pairs",
        f"QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 4, POSITIVE POINT 10 NEGATIVE POINT 3) FROM {COL} USING dense LIMIT 3",
        execute=args.execute)
    run("Discover",
        f"QUERY DISCOVER TARGET POINT 1 CONTEXT (POSITIVE POINT 12 NEGATIVE POINT 4) FROM {COL} USING dense LIMIT 3",
        execute=args.execute)

    # -- CTE Prefetch DAG --
    print("\n[9] CTE-based Prefetch DAGs")
    run("Prefetch RRF (dense + sparse)",
        f"WITH a AS (QUERY 'emergency neurological' FROM {COL} USING dense LIMIT 10), b AS (QUERY 'emergency neurological' FROM {COL} USING sparse LIMIT 10) QUERY FUSION RRF FROM {COL} PREFETCH (a, b) LIMIT 3",
        execute=args.execute)
    run("Prefetch RRF + per-prefetch filter + score threshold",
        f"WITH a AS (QUERY 'emergency neurological' FROM {COL} USING dense WHERE priority = 'high' LIMIT 20), b AS (QUERY 'emergency neurological' FROM {COL} USING sparse LIMIT 20) QUERY FUSION RRF FROM {COL} PREFETCH (a SCORE THRESHOLD 0.3, b SCORE THRESHOLD 0.1) LIMIT 3",
        execute=args.execute)
    run("Prefetch RRF + RRF params",
        f"WITH a AS (QUERY 'emergency neurological' FROM {COL} USING dense WHERE priority = 'high' LIMIT 10), b AS (QUERY 'emergency neurological' FROM {COL} USING sparse LIMIT 10) QUERY FUSION RRF FROM {COL} PREFETCH (a, b) PARAMS (rrf_k = 10, rrf_weights = [0.7, 0.3]) LIMIT 3",
        execute=args.execute)

    # -- Mutations --
    print("\n[10] Mutations")
    run("Update payload by ID",
        f"UPDATE {COL} SET PAYLOAD = {{'status': 'reviewed', 'care_path': 'stroke-alert'}} WHERE id = 1",
        execute=args.execute)
    run("Update payload by filter",
        f"UPDATE {COL} SET PAYLOAD = {{'status': 'archived'}} WHERE status = 'discharged'",
        execute=args.execute)
    run("Delete by filter",
        f"DELETE FROM {COL} WHERE status = 'archived'",
        execute=args.execute)

    # -- ORDER BY & Output Selectors --
    print("\n[11] ORDER BY & Output Selection")
    run("ORDER BY year DESC",
        f"QUERY ORDER BY year DESC FROM {COL} LIMIT 5",
        execute=args.execute)
    run("WITH PAYLOAD false",
        f"QUERY TEXT 'stroke' FROM {COL} USING dense {QRESCORE} WITH PAYLOAD false LIMIT 3",
        execute=args.execute)
    run("WITH PAYLOAD INCLUDE",
        f"QUERY TEXT 'emergency' FROM {COL} USING dense {QRESCORE} WITH PAYLOAD INCLUDE ('specialty', 'priority') LIMIT 3",
        execute=args.execute)

    # -- Point Access --
    print("\n[12] Point Access")
    run("Point lookup by ID",
        f"QUERY POINTS (2) FROM {COL} WITH PAYLOAD true",
        execute=args.execute)
    run("Scroll all",
        f"SCROLL FROM {COL} LIMIT 3",
        execute=args.execute)
    run("Scroll filtered",
        f"SCROLL FROM {COL} WHERE priority = 'high' LIMIT 3",
        execute=args.execute)
    run("Scroll WITHOUT vectors",
        f"SCROLL FROM {COL} WITH VECTOR false LIMIT 3",
        execute=args.execute)
    run("COUNT exact",
        f"COUNT FROM {COL} WHERE priority = 'high' WITH (exact = true)",
        execute=args.execute)
    run("DELETE PAYLOAD keys",
        f"DELETE PAYLOAD care_path FROM {COL} WHERE id = 1",
        execute=args.execute)

    # -- Operations --
    print("\n[13] Operations")
    run("SHOW COLLECTIONS", "SHOW COLLECTIONS", execute=args.execute)
    run("SHOW COLLECTION", f"SHOW COLLECTION {COL}", execute=args.execute)
    run("EXPLAIN",
        f"QUERY TEXT 'stroke' FROM {COL} USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT 3",
        execute=args.execute, explain=True)

    # -- Cleanup --
    if args.execute and not args.keep:
        try:
            qql_exec(f"DROP COLLECTION {COL}")
            print(f"\n[cleanup] Dropped {COL}")
        except:
            pass

    print("\nDone.")


if __name__ == "__main__":
    main()
