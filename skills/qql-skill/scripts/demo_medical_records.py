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
        f"CREATE COLLECTION {COLLECTION} (dense VECTOR (384, COSINE), sparse SPARSE) HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)"))

    for field, ftype in [
        ("specialty", "keyword"), ("priority", "keyword"), ("status", "keyword"),
        ("year", "integer"), ("patient_id", "keyword"),
        ("diagnosis", "text WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true, phrase_matching = true)"),
    ]:
        stmts.append((f"index-{field}",
            f"CREATE INDEX ON COLLECTION {COLLECTION} FOR {field} TYPE {ftype}"))

    # Upserts
    for pid, text, patient_id, specialty, priority, diagnosis, status, year in RECORDS:
        stmts.append((f"upsert-{pid}",
            f"""UPSERT INTO {COLLECTION} VALUES {{
  id: {pid},
  text: '{text}',
  patient_id: '{patient_id}',
  specialty: '{specialty}',
  priority: '{priority}',
  diagnosis: '{diagnosis}',
  status: '{status}',
  year: {year}
}} USING HYBRID"""))

    # Search modes
    stmts.append(("search-hybrid",
        f"QUERY HYBRID TEXT 'acute stroke weakness slurred speech' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} LIMIT 3"))
    stmts.append(("search-hybrid-dbsf",
        f"QUERY HYBRID TEXT 'acute stroke weakness slurred speech' DENSE dense SPARSE sparse FUSION DBSF FROM {COLLECTION} LIMIT 3"))
    stmts.append(("search-sparse",
        f"QUERY 'fever cough antibiotics consolidation' FROM {COLLECTION} USING sparse LIMIT 3"))
    stmts.append(("search-exact",
        f"QUERY 'chest pain troponin elevated' FROM {COLLECTION} USING dense PARAMS (exact = true) LIMIT 3"))

    # Parameterized RRF
    stmts.append(("search-rrf-params",
        f"QUERY HYBRID TEXT 'emergency critical neurological' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} WITH (rrf_k = 30, rrf_weights = [0.7, 0.3]) LIMIT 3"))

    # MMR
    stmts.append(("search-mmr",
        f"QUERY MMR TEXT 'acute neurological emergency triage' DIVERSITY 0.5 CANDIDATES 20 FROM {COLLECTION} USING dense LIMIT 5"))

    # Score threshold + offset
    stmts.append(("search-score-threshold",
        f"QUERY 'patient treatment' FROM {COLLECTION} USING dense SCORE THRESHOLD 0.3 LIMIT 10"))
    stmts.append(("search-offset",
        f"QUERY 'patient diagnosis' FROM {COLLECTION} USING dense LIMIT 3 OFFSET 3"))

    # Filters
    stmts.append(("filter-specialty",
        f"QUERY 'headache neurological' FROM {COLLECTION} USING dense WHERE specialty = 'neurology' LIMIT 3"))
    stmts.append(("filter-priority-in",
        f"QUERY 'chest pain cardiac' FROM {COLLECTION} USING dense WHERE priority IN ('high', 'medium') LIMIT 3"))
    stmts.append(("filter-status",
        f"QUERY 'pain' FROM {COLLECTION} USING dense WHERE status = 'admitted' LIMIT 3"))
    stmts.append(("filter-combined",
        f"QUERY 'cardiac emergency chest' FROM {COLLECTION} USING dense WHERE priority = 'high' AND status = 'admitted' LIMIT 3"))
    stmts.append(("filter-range",
        f"QUERY 'patient' FROM {COLLECTION} USING dense WHERE year BETWEEN 2024 AND 2026 LIMIT 3"))
    stmts.append(("filter-match-phrase",
        f"QUERY 'chest pain' FROM {COLLECTION} USING dense WHERE diagnosis MATCH PHRASE 'chest pain' LIMIT 3"))

    # Query-time params
    stmts.append(("search-hnsw-ef",
        f"QUERY 'stroke rehabilitation' FROM {COLLECTION} USING dense PARAMS (hnsw_ef = 256) LIMIT 3"))
    stmts.append(("search-acorn",
        f"QUERY 'emergency triage' FROM {COLLECTION} USING dense WHERE specialty = 'neurology' PARAMS (acorn = true) LIMIT 3"))

    # Grouped
    stmts.append(("group-by-specialty",
        f"QUERY 'acute neurological emergency' FROM {COLLECTION} USING dense GROUP BY specialty SIZE 2 LIMIT 4"))
    stmts.append(("group-by-priority",
        f"QUERY 'patient treatment' FROM {COLLECTION} USING dense GROUP BY priority SIZE 2 LIMIT 4"))
    stmts.append(("grouped-with-params",
        f"QUERY 'critical care' FROM {COLLECTION} USING dense PARAMS (hnsw_ef = 128) GROUP BY specialty SIZE 2 LIMIT 4"))

    # Recommend
    stmts.append(("recommend-single",
        f"QUERY RECOMMEND POSITIVE (414) FROM {COLLECTION} USING dense LIMIT 3"))
    stmts.append(("recommend-multi",
        f"QUERY RECOMMEND POSITIVE (414, 424) NEGATIVE (418) FROM {COLLECTION} USING dense LIMIT 3"))
    stmts.append(("recommend-strategy",
        f"QUERY RECOMMEND POSITIVE (416, 420) STRATEGY best_score FROM {COLLECTION} USING dense LIMIT 3"))

    # Context + Discover
    stmts.append(("context-pairs",
        f"QUERY CONTEXT (POSITIVE POINT 414 NEGATIVE POINT 418, POSITIVE POINT 420 NEGATIVE POINT 415) FROM {COLLECTION} USING dense LIMIT 3"))
    stmts.append(("discover",
        f"QUERY DISCOVER TARGET POINT 414 CONTEXT (POSITIVE POINT 424 NEGATIVE POINT 418) FROM {COLLECTION} USING dense LIMIT 3"))

    # Complex Score Boosting Formulas & Relevance Feedback
    stmts.append(("formula-linear-boost",
        f"""WITH a AS (QUERY 'acute stroke weakness' FROM {COLLECTION} USING dense LIMIT 20)
QUERY FORMULA (score * 0.7 + year * 0.001) FROM {COLLECTION} PREFETCH (a) LIMIT 3"""))
    stmts.append(("formula-conditional-case-boost",
        f"""WITH a AS (QUERY 'critical chest pain' FROM {COLLECTION} USING dense LIMIT 20)
QUERY FORMULA (CASE WHEN priority = 'high' THEN score * 2.5 ELSE score END) FROM {COLLECTION} PREFETCH (a) LIMIT 3"""))
    stmts.append(("formula-decay-geo-boost",
        f"""WITH a AS (QUERY 'emergency stroke triage' FROM {COLLECTION} USING dense LIMIT 20)
QUERY FORMULA (score * GAUSS_DECAY(GEO_DISTANCE(48.8566, 2.3522, location), 0, 5000, 0.5)) DEFAULTS (location = {{lat: 48.8566, lon: 2.3522}}) FROM {COLLECTION} PREFETCH (a) LIMIT 3"""))
    stmts.append(("formula-math-defaults-boost",
        f"""WITH a AS (QUERY 'pulmonology asthma' FROM {COLLECTION} USING dense LIMIT 20)
QUERY FORMULA (SQRT(score) * LOG(year + 1)) DEFAULTS (year = 2024) FROM {COLLECTION} PREFETCH (a) LIMIT 3"""))
    stmts.append(("relevance-feedback-naive",
        f"QUERY RELEVANCE FEEDBACK TARGET 'stroke' FEEDBACK ((POINT 414, 0.9), (POINT 418, -0.4)) STRATEGY NAIVE (a = 1.0, b = 0.75, c = 0.25) FROM {COLLECTION} USING dense LIMIT 3"))

    # CTE-based Prefetch DAG
    stmts.append(("prefetch-rrf",
        f"""WITH a AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING dense LIMIT 10), b AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING sparse LIMIT 10)
QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a, b) LIMIT 3"""))
    stmts.append(("prefetch-rrf-per-filter",
        f"""WITH a AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING dense WHERE priority = 'high' SCORE THRESHOLD 0.3 LIMIT 20), b AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING sparse SCORE THRESHOLD 0.1 LIMIT 20)
QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a, b) PARAMS (rrf_k = 20, rrf_weights = [0.6, 0.4]) LIMIT 3"""))
    stmts.append(("prefetch-rrf-params",
        f"""WITH a AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING dense WHERE priority = 'high' LIMIT 10), b AS (QUERY 'emergency critical neurological' FROM {COLLECTION} USING sparse PARAMS (exact = true) LIMIT 10)
QUERY FUSION RRF FROM {COLLECTION} PREFETCH (a, b) PARAMS (rrf_k = 10, rrf_weights = [0.7, 0.3]) LIMIT 3"""))

    # Update
    stmts.append(("update-payload",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{status: 'reviewed', care_path: 'stroke-alert'}} WHERE id = 414"))
    stmts.append(("update-filter",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{status: 'archived'}} WHERE status = 'discharged'"))

    # Select / Scroll
    stmts.append(("select-by-id",
        f"QUERY POINTS (416) FROM {COLLECTION} WITH PAYLOAD true"))
    stmts.append(("scroll-all",
        f"SCROLL FROM {COLLECTION} LIMIT 3"))
    stmts.append(("scroll-filtered",
        f"SCROLL FROM {COLLECTION} WHERE priority = 'high' LIMIT 3"))

    # ORDER BY — paginate without similarity score
    stmts.append(("order-by-year",
        f"QUERY ORDER BY year DESC FROM {COLLECTION} LIMIT 5"))

    # WITH PAYLOAD / WITH VECTOR — field selection
    stmts.append(("payload-exclude",
        f"QUERY 'acute stroke' FROM {COLLECTION} USING dense WITH PAYLOAD EXCLUDE (patient_id, diagnosis) LIMIT 3"))

    # SAMPLE — random point sampling
    stmts.append(("sample-random",
        f"QUERY SAMPLE RANDOM FROM {COLLECTION} LIMIT 5"))

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

