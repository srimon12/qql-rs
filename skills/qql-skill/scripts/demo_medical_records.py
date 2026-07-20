#!/usr/bin/env python3
from __future__ import annotations

import argparse
from _qql_cli import drop_collection_if_exists, execute_json, print_result

COLLECTION = "medical_records_demo"

RECORDS = [
    (414, "Patient presents with sudden right-sided weakness and slurred speech. CT brain confirms left MCA infarct. Thrombolysis initiated within treatment window.", "PT-00414", "neurology", "high", "Acute ischemic stroke", "admitted", 2026),
    (415, "Patient with high-grade fever, chills, and productive cough. Chest X-ray shows right lower lobe consolidation. Started on broad-spectrum antibiotics.", "PT-00415", "pulmonology", "medium", "Community-acquired pneumonia", "reviewed", 2025),
    (416, "Patient with crushing substernal chest pain radiating to jaw. ECG shows ST depression in leads V4-V6. Troponin I elevated at 2.4 ng/mL.", "PT-00416", "cardiology", "high", "Non-ST elevation myocardial infarction", "admitted", 2026),
    (417, "RLQ pain with positive McBurney sign. WBC elevated at 14,500. CT confirms acute appendicitis with no perforation. Patient scheduled for laparoscopic appendectomy.", "PT-00417", "surgery", "medium", "Acute appendicitis", "preoperative", 2024),
    (418, "Severe unilateral headache with photophobia and phonophobia. Previous similar episodes. Patient reports nausea. Migraine without aura.", "PT-00418", "neurology", "low", "Migraine", "discharged", 2023),
    (419, "Acute exacerbation of asthma with wheezing and dyspnea. Peak flow at 40% predicted. Nebulized bronchodilators and systemic corticosteroids started.", "PT-00419", "pulmonology", "high", "Acute asthma exacerbation", "admitted", 2026),
    (420, "Fever, tachycardia, hypotension, and elevated lactate at 4.2 mmol/L. Blood cultures drawn. Broad-spectrum antibiotics and aggressive fluid resuscitation initiated.", "PT-00420", "internal-medicine", "high", "Sepsis", "admitted", 2025),
    (421, "Fall from height resulting in closed comminuted fracture of the left tibia. X-ray confirms displaced fracture. Open reduction internal fixation planned.", "PT-00421", "orthopedics", "medium", "Tibial fracture", "preoperative", 2025),
    (422, "Patient with poorly controlled type 2 diabetes. HbA1c at 10.2%. Started on insulin regimen and referred to endocrinology for optimization.", "PT-00422", "endocrinology", "medium", "Uncontrolled diabetes mellitus type 2", "reviewed", 2024),
    (423, "Epigastric pain radiating to the back. Lipase elevated at 5x normal. CT shows acute edematous pancreatitis. NPO and IV fluids started.", "PT-00423", "gastroenterology", "medium", "Acute pancreatitis", "admitted", 2024),
    (424, "Patient with severe headache, neck stiffness, and fever of 39.2C. Lumbar puncture performed. Empiric IV antibiotics started pending CSF culture results.", "PT-00424", "neurology", "high", "Suspected bacterial meningitis", "admitted", 2026),
    (425, "Left lower extremity swelling and pain. Doppler ultrasound confirms deep vein thrombosis in the popliteal vein. Therapeutic anticoagulation with heparin initiated.", "PT-00425", "vascular", "medium", "Deep vein thrombosis", "reviewed", 2025),
]


def build_statements():
    stmts = []

    # Schema
    stmts.append(("create-collection",
        f"CREATE COLLECTION {COLLECTION} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)"))

    for field, ftype in [
        ("specialty", "keyword"), ("priority", "keyword"), ("status", "keyword"),
        ("year", "integer"), ("patient_id", "keyword"),
        ("diagnosis", "text WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true, phrase_matching = true)"),
    ]:
        stmts.append((f"index-{field}",
            f"CREATE INDEX ON COLLECTION {COLLECTION} FOR {field} TYPE {ftype}"))

    # Inserts
    for pid, text, patient_id, specialty, priority, diagnosis, status, year in RECORDS:
        stmts.append((f"insert-{pid}",
            f"""INSERT INTO {COLLECTION} VALUES {{
  'id': {pid},
  'text': '{text}',
  'patient_id': '{patient_id}',
  'specialty': '{specialty}',
  'priority': '{priority}',
  'diagnosis': '{diagnosis}',
  'status': '{status}',
  'year': {year}
}} USING HYBRID"""))

    # Search modes
    stmts.append(("search-hybrid",
        f"QUERY 'acute stroke weakness slurred speech' FROM {COLLECTION} LIMIT 3 USING HYBRID"))
    stmts.append(("search-hybrid-dbsf",
        f"QUERY 'acute stroke weakness slurred speech' FROM {COLLECTION} LIMIT 3 USING HYBRID FUSION DBSF"))
    stmts.append(("search-sparse",
        f"QUERY 'fever cough antibiotics consolidation' FROM {COLLECTION} LIMIT 3 USING SPARSE"))
    stmts.append(("search-exact",
        f"QUERY 'chest pain troponin elevated' FROM {COLLECTION} LIMIT 3 EXACT"))

    # Parameterized RRF
    stmts.append(("search-rrf-params",
        f"QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])"))

    # MMR
    stmts.append(("search-mmr",
        f"QUERY 'acute neurological emergency triage' FROM {COLLECTION} LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)"))

    # Score threshold + offset
    stmts.append(("search-score-threshold",
        f"QUERY 'patient treatment' FROM {COLLECTION} LIMIT 10 SCORE THRESHOLD 0.3"))
    stmts.append(("search-offset",
        f"QUERY 'patient diagnosis' FROM {COLLECTION} LIMIT 3 OFFSET 3"))

    # Filters
    stmts.append(("filter-specialty",
        f"QUERY 'headache neurological' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE specialty = 'neurology'"))
    stmts.append(("filter-priority-in",
        f"QUERY 'chest pain cardiac' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE priority IN ('high', 'medium')"))
    stmts.append(("filter-status",
        f"QUERY 'pain' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE status = 'admitted'"))
    stmts.append(("filter-combined",
        f"QUERY 'cardiac emergency chest' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE priority = 'high' AND status = 'admitted'"))
    stmts.append(("filter-range",
        f"QUERY 'patient' FROM {COLLECTION} LIMIT 3 WHERE year BETWEEN 2024 AND 2026"))
    stmts.append(("filter-match-phrase",
        f"QUERY 'chest pain' FROM {COLLECTION} LIMIT 3 WHERE diagnosis MATCH PHRASE 'chest pain'"))

    # Query-time params
    stmts.append(("search-hnsw-ef",
        f"QUERY 'stroke rehabilitation' FROM {COLLECTION} LIMIT 3 WITH (hnsw_ef = 256)"))
    stmts.append(("search-acorn",
        f"QUERY 'emergency triage' FROM {COLLECTION} LIMIT 3 WHERE specialty = 'neurology' WITH (acorn = true)"))

    # Grouped
    stmts.append(("group-by-specialty",
        f"QUERY 'acute neurological emergency' FROM {COLLECTION} LIMIT 4 GROUP BY 'specialty' GROUP_SIZE 2"))
    stmts.append(("group-by-priority",
        f"QUERY 'patient treatment' FROM {COLLECTION} LIMIT 4 USING HYBRID GROUP BY 'priority' GROUP_SIZE 2"))
    stmts.append(("grouped-with-params",
        f"QUERY 'critical care' FROM {COLLECTION} LIMIT 4 USING HYBRID WITH (hnsw_ef = 128) GROUP BY 'specialty' GROUP_SIZE 2"))

    # Recommend
    stmts.append(("recommend-single",
        f"QUERY RECOMMEND WITH (positive = (414)) FROM {COLLECTION} LIMIT 3"))
    stmts.append(("recommend-multi",
        f"QUERY RECOMMEND WITH (positive = (414, 424), negative = (418)) FROM {COLLECTION} LIMIT 3"))
    stmts.append(("recommend-strategy",
        f"QUERY RECOMMEND WITH (positive = (416, 420)) FROM {COLLECTION} STRATEGY 'best_score' LIMIT 3"))

    # Context + Discover
    stmts.append(("context-pairs",
        f"QUERY CONTEXT PAIRS (414, 418), (420, 415) FROM {COLLECTION} LIMIT 3"))
    stmts.append(("discover",
        f"QUERY DISCOVER TARGET 414 CONTEXT PAIRS (424, 418) FROM {COLLECTION} LIMIT 3"))

    # CTE-based Prefetch DAG
    stmts.append(("prefetch-rrf",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 10), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 10)
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a, b) FUSION RRF"""))
    stmts.append(("prefetch-rrf-per-filter",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 20), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 20)
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.3, b SCORE THRESHOLD 0.1) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])"""))
    stmts.append(("prefetch-rrf-params",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 10 WHERE priority = 'high'), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 10)
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a, b) FUSION RRF WITH (rrf_k = 10, rrf_weights = [0.7, 0.3])"""))

    # Update
    stmts.append(("update-payload",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{'status': 'reviewed', 'care_path': 'stroke-alert'}} WHERE id = 414"))
    stmts.append(("update-filter",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{'status': 'archived'}} WHERE status = 'discharged'"))

    # Select / Scroll
    stmts.append(("select-by-id",
        f"SELECT * FROM {COLLECTION} WHERE id = 416"))
    stmts.append(("scroll-all",
        f"SCROLL FROM {COLLECTION} LIMIT 3"))
    stmts.append(("scroll-filtered",
        f"SCROLL FROM {COLLECTION} WHERE priority = 'high' LIMIT 3"))

    # ORDER BY — paginate without similarity score
    stmts.append(("order-by-year",
        f"QUERY ORDER BY year DESC FROM {COLLECTION} LIMIT 5"))

    # WITH PAYLOAD / WITH VECTORS — field selection
    stmts.append(("payload-exclude",
        f"QUERY 'acute stroke' FROM {COLLECTION} LIMIT 3 USING HYBRID WITH PAYLOAD (exclude = ['patient_id', 'diagnosis'])"))

    # SAMPLE — random point sampling
    stmts.append(("sample-random",
        f"QUERY SAMPLE FROM {COLLECTION} LIMIT 5"))

    # BOOST — score boosting with formula
    stmts.append(("boost-arithmetic",
        f"QUERY 'emergency critical' FROM {COLLECTION} LIMIT 5 USING DENSE BOOST (year * 0.001)"))
    stmts.append(("boost-conditional",
        f"QUERY 'patient treatment' FROM {COLLECTION} LIMIT 5 USING DENSE BOOST (CASE WHEN priority = 'high' THEN 2.0 ELSE 1.0 END)"))

    # Delete
    stmts.append(("delete-by-filter",
        f"DELETE FROM {COLLECTION} WHERE status = 'archived'"))

    # Show
    stmts.append(("show-collections", "SHOW COLLECTIONS"))
    stmts.append(("show-collection", f"SHOW COLLECTION {COLLECTION}"))

    return stmts


def main() -> None:
    parser = argparse.ArgumentParser(description="QQL Medical Records — full E2E showcase")
    parser.add_argument("--execute", action="store_true", help="Run against Qdrant")
    parser.add_argument("--keep", action="store_true", help="Keep collection after run")
    parser.add_argument("--rerank", action="store_true", help="Include rerank tests (cloud only)")
    args = parser.parse_args()

    statements = build_statements()

    if args.rerank:
        statements.insert(0, ("create-collection",
            f"CREATE COLLECTION {COLLECTION} HYBRID RERANK WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)"))
        statements.insert(len(statements) - 2, ("search-hybrid-rerank",
            f"QUERY 'acute stroke weakness slurred speech' FROM {COLLECTION} LIMIT 3 USING HYBRID RERANK"))
        statements.insert(len(statements) - 2, ("search-sparse-rerank",
            f"QUERY 'chest pain troponin' FROM {COLLECTION} LIMIT 3 USING SPARSE RERANK"))

    try:
        if args.execute:
            drop_collection_if_exists(COLLECTION)

        for label, statement in statements:
            print(f"[{label}]")
            print(statement)
            print()

            if not args.execute:
                continue

            try:
                result = execute_json(statement)
                print_result(label, result, limit=3)
            except Exception as exc:
                print(f"  ERROR: {exc}")
                print()

    finally:
        if args.execute and not args.keep:
            try:
                result = execute_json(f"DROP COLLECTION {COLLECTION}")
                print(f"[cleanup]\n{result.message}\n")
            except Exception as exc:
                print(f"[cleanup]\ncleanup failed: {exc}\n")


if __name__ == "__main__":
    main()

