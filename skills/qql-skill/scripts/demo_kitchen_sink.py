#!/usr/bin/env python3
from __future__ import annotations

import argparse
from _qql_cli import drop_collection_if_exists, execute_json, print_result

COLLECTION = "qql_kitchen_sink"

IDS = {
    "stroke": "123e4567-e89b-12d3-a456-426614174001",
    "stemi": "123e4567-e89b-12d3-a456-426614174002",
    "pneumonia": "123e4567-e89b-12d3-a456-426614174003",
    "headache": "123e4567-e89b-12d3-a456-426614174004",
    "appendicitis": "123e4567-e89b-12d3-a456-426614174005",
    "migraine": "123e4567-e89b-12d3-a456-426614174006",
    "fracture": "123e4567-e89b-12d3-a456-426614174007",
    "diabetes": "123e4567-e89b-12d3-a456-426614174008",
    "asthma": "123e4567-e89b-12d3-a456-426614174009",
    "sepsis": "123e4567-e89b-12d3-a456-426614174010",
    "copd": "123e4567-e89b-12d3-a456-426614174011",
    "meningitis": "123e4567-e89b-12d3-a456-426614174012",
    "dvt": "123e4567-e89b-12d3-a456-426614174013",
    "pancreatitis": "123e4567-e89b-12d3-a456-426614174014",
    "renal": "123e4567-e89b-12d3-a456-426614174015",
}

RECORDS = [
    ("stroke", "Patient presents with sudden right-sided weakness and slurred speech. CT confirms left MCA infarct. Thrombolysis initiated within the treatment window.", "neurology", "high", "Acute ischemic stroke", "admitted", 2026, 9.7),
    ("stemi", "Patient with crushing chest pain radiating to the left arm. ECG shows ST elevation in V1-V4. Troponin elevated. Emergency catheterization planned.", "cardiology", "high", "STEMI", "admitted", 2025, 9.2),
    ("pneumonia", "High-grade fever, productive cough, and right lower lobe consolidation on chest X-ray. Started on IV antibiotics and supplemental oxygen.", "pulmonology", "medium", "Community-acquired pneumonia", "reviewed", 2024, 7.1),
    ("headache", "Mild tension headache improved with rest and hydration. No focal neurological deficits observed. Patient discharged with follow-up.", "general-medicine", "low", "Tension headache", "discharged", 2023, 4.5),
    ("appendicitis", "RLQ pain with positive McBurney sign. WBC elevated at 14,500. CT confirms acute appendicitis with no perforation. Laparoscopic appendectomy scheduled.", "surgery", "medium", "Acute appendicitis", "preoperative", 2024, 7.8),
    ("migraine", "Severe unilateral headache with photophobia and phonophobia. Previous similar episodes. Migraine without aura. IV sumatriptan administered.", "neurology", "low", "Migraine", "discharged", 2023, 5.2),
    ("fracture", "Fall from height resulting in closed comminuted fracture of the left tibia. X-ray confirms displaced fracture. ORIF planned.", "orthopedics", "medium", "Tibial fracture", "preoperative", 2025, 6.8),
    ("diabetes", "Patient with poorly controlled type 2 diabetes. HbA1c at 10.2%. Started on insulin regimen and referred to endocrinology.", "endocrinology", "medium", "Uncontrolled diabetes mellitus type 2", "reviewed", 2024, 5.9),
    ("asthma", "Acute exacerbation of asthma with wheezing and dyspnea. Peak flow at 40% predicted. Nebulized bronchodilators and systemic corticosteroids started.", "pulmonology", "high", "Acute asthma exacerbation", "admitted", 2026, 8.1),
    ("sepsis", "Fever, tachycardia, hypotension, and elevated lactate. Blood cultures drawn. Broad-spectrum antibiotics and fluid resuscitation initiated. ICU transfer.", "internal-medicine", "high", "Sepsis", "admitted", 2025, 9.5),
    ("copd", "Chronic obstructive pulmonary disease exacerbation with increased dyspnea and purulent sputum. Supplemental oxygen and bronchodilators started.", "pulmonology", "medium", "COPD exacerbation", "reviewed", 2024, 6.3),
    ("meningitis", "Patient with severe headache, neck stiffness, and fever. Lumbar puncture performed. Empiric antibiotics started pending CSF results.", "neurology", "high", "Suspected bacterial meningitis", "admitted", 2026, 9.0),
    ("dvt", "Left lower extremity swelling and pain. Doppler ultrasound confirms deep vein thrombosis in the popliteal vein. Anticoagulation initiated.", "vascular", "medium", "Deep vein thrombosis", "reviewed", 2025, 6.5),
    ("pancreatitis", "Epigastric pain radiating to the back. Lipase elevated at 5x normal. CT shows acute edematous pancreatitis. NPO and IV fluids started.", "gastroenterology", "medium", "Acute pancreatitis", "admitted", 2024, 7.0),
    ("renal", "Flank pain and hematuria. CT KUB shows 8mm right ureteral stone with hydronephrosis. Urology consulted for possible lithotripsy.", "urology", "medium", "Ureteral calculus", "reviewed", 2025, 6.1),
]

def build_statements():
    stmts = []

    # --- Schema ---
    stmts.append(("create-collection",
        f"CREATE COLLECTION {COLLECTION} HYBRID WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)"))

    for field, ftype in [
        ("specialty", "keyword"), ("priority", "keyword"), ("status", "keyword"),
        ("year", "integer"), ("score", "float"),
        ("patient_id", "keyword"),
        ("diagnosis", "text WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true, phrase_matching = true)"),
    ]:
        stmts.append((f"index-{field}",
            f"CREATE INDEX ON COLLECTION {COLLECTION} FOR {field} TYPE {ftype}"))

    # --- Inserts ---
    for key, text, specialty, priority, diagnosis, status, year, score in RECORDS:
        stmts.append((f"insert-{key}",
            f"""INSERT INTO {COLLECTION} VALUES {{
  'id': '{IDS[key]}',
  'text': '{text}',
  'patient_id': 'PT-{key.upper()[:4]}',
  'specialty': '{specialty}',
  'priority': '{priority}',
  'diagnosis': '{diagnosis}',
  'status': '{status}',
  'year': {year},
  'score': {score}
}} USING HYBRID"""))

    # --- Bulk insert test (comma-separated VALUES) ---
    stmts.append(("bulk-insert",
        f"""INSERT INTO {COLLECTION} VALUES
  {{'text': 'Routine follow-up for hypertension. Blood pressure well controlled on current medication.', 'specialty': 'cardiology', 'priority': 'low', 'status': 'reviewed', 'year': 2026, 'diagnosis': 'Hypertension follow-up', 'patient_id': 'PT-BULK1', 'score': 3.0}},
  {{'text': 'Post-operative wound check after laparoscopic cholecystectomy. Incision healing well, no signs of infection.', 'specialty': 'surgery', 'priority': 'low', 'status': 'discharged', 'year': 2026, 'diagnosis': 'Post-op wound check', 'patient_id': 'PT-BULK2', 'score': 3.5}}
USING HYBRID"""))

    # --- Dense search ---
    stmts.append(("search-dense",
        f"QUERY 'acute stroke weakness slurred speech' FROM {COLLECTION} LIMIT 3"))
    stmts.append(("search-dense-exact",
        f"QUERY 'acute stroke weakness slurred speech' FROM {COLLECTION} LIMIT 3 EXACT"))
    stmts.append(("search-dense-by-id",
        f"QUERY '{IDS['stroke']}' FROM {COLLECTION} LIMIT 1"))

    # --- Hybrid search ---
    stmts.append(("search-hybrid",
        f"QUERY 'chest pain radiating arm troponin' FROM {COLLECTION} LIMIT 3 USING HYBRID"))
    stmts.append(("search-hybrid-dbsf",
        f"QUERY 'chest pain radiating arm troponin' FROM {COLLECTION} LIMIT 3 USING HYBRID FUSION DBSF"))

    # --- Parameterized RRF ---
    stmts.append(("search-hybrid-rrf-params",
        f"QUERY 'emergency critical care' FROM {COLLECTION} LIMIT 3 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])"))

    # --- Sparse search ---
    stmts.append(("search-sparse",
        f"QUERY 'fever cough consolidation antibiotics' FROM {COLLECTION} LIMIT 3 USING SPARSE"))

    # --- MMR ---
    stmts.append(("search-mmr",
        f"QUERY 'neurological emergency triage' FROM {COLLECTION} LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)"))

    # --- Score threshold ---
    stmts.append(("search-score-threshold",
        f"QUERY 'emergency critical care' FROM {COLLECTION} LIMIT 10 SCORE THRESHOLD 0.3"))

    # --- Offset ---
    stmts.append(("search-offset",
        f"QUERY 'patient treatment diagnosis' FROM {COLLECTION} LIMIT 3 OFFSET 3"))

    # --- Filters ---
    stmts.append(("filter-equality",
        f"QUERY 'emergency' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE specialty = 'neurology'"))
    stmts.append(("filter-in",
        f"QUERY 'chest pain cardiac' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE priority IN ('high', 'medium')"))
    stmts.append(("filter-between",
        f"QUERY 'patient' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE year BETWEEN 2024 AND 2026"))
    stmts.append(("filter-and",
        f"QUERY 'emergency critical' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE priority = 'high' AND status = 'admitted'"))
    stmts.append(("filter-or",
        f"QUERY 'breathing difficulty' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE specialty = 'pulmonology' OR specialty = 'cardiology'"))
    stmts.append(("filter-not-in",
        f"QUERY 'patient' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE status NOT IN ('discharged')"))
    stmts.append(("filter-combined",
        f"QUERY 'acute emergency' FROM {COLLECTION} LIMIT 3 USING HYBRID WHERE priority IN ('high', 'medium') AND status = 'admitted' AND year >= 2024"))
    stmts.append(("filter-null-check",
        f"QUERY 'patient' FROM {COLLECTION} LIMIT 3 WHERE diagnosis IS NOT NULL"))
    stmts.append(("filter-match-phrase",
        f"QUERY 'chest pain' FROM {COLLECTION} LIMIT 3 WHERE diagnosis MATCH PHRASE 'chest pain'"))

    # --- Query-time params ---
    stmts.append(("search-hnsw-ef",
        f"QUERY 'stroke rehabilitation' FROM {COLLECTION} LIMIT 3 WITH (hnsw_ef = 256)"))
    stmts.append(("search-acorn",
        f"QUERY 'emergency triage' FROM {COLLECTION} LIMIT 3 WHERE specialty = 'neurology' WITH (acorn = true)"))
    stmts.append(("search-indexed-only",
        f"QUERY 'patient' FROM {COLLECTION} LIMIT 3 WITH (indexed_only = true)"))

    # --- Grouped search ---
    stmts.append(("group-by-specialty",
        f"QUERY 'emergency acute care' FROM {COLLECTION} LIMIT 4 GROUP BY 'specialty' GROUP_SIZE 2"))
    stmts.append(("group-by-priority",
        f"QUERY 'patient treatment' FROM {COLLECTION} LIMIT 4 USING HYBRID GROUP BY 'priority' GROUP_SIZE 2"))
    stmts.append(("grouped-with-params",
        f"QUERY 'critical care escalation' FROM {COLLECTION} LIMIT 4 USING HYBRID WITH (hnsw_ef = 128) GROUP BY 'specialty' GROUP_SIZE 2"))

    # --- Recommend ---
    stmts.append(("recommend-stroke",
        f"QUERY RECOMMEND WITH (positive = ('{IDS['stroke']}')) FROM {COLLECTION} LIMIT 3"))
    stmts.append(("recommend-multi",
        f"QUERY RECOMMEND WITH (positive = ('{IDS['stroke']}', '{IDS['meningitis']}'), negative = ('{IDS['headache']}')) FROM {COLLECTION} LIMIT 3"))
    stmts.append(("recommend-strategy",
        f"QUERY RECOMMEND WITH (positive = ('{IDS['stemi']}', '{IDS['sepsis']}')) FROM {COLLECTION} STRATEGY 'best_score' LIMIT 3"))

    # --- Context ---
    stmts.append(("context-pairs",
        f"QUERY CONTEXT PAIRS ('{IDS['stroke']}', '{IDS['headache']}'), ('{IDS['sepsis']}', '{IDS['pneumonia']}') FROM {COLLECTION} LIMIT 3"))

    # --- Discover ---
    stmts.append(("discover",
        f"QUERY DISCOVER TARGET '{IDS['stroke']}' CONTEXT PAIRS ('{IDS['meningitis']}', '{IDS['headache']}') FROM {COLLECTION} LIMIT 3"))

    # --- CTE-based Prefetch DAG ---
    stmts.append(("prefetch-rrf",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 10), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 10)
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a, b) FUSION RRF"""))
    stmts.append(("prefetch-rrf-per-filter",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 20), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 20)
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.3, b SCORE THRESHOLD 0.1) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])"""))
    stmts.append(("prefetch-rrf-params",
        f"""WITH a AS (QUERY 'emergency critical neurological' USING dense LIMIT 10 WHERE priority = 'high'), b AS (QUERY 'emergency critical neurological' USING sparse LIMIT 10 WITH (exact = true))
QUERY 'emergency critical neurological' FROM {COLLECTION} LIMIT 3 PREFETCH (a, b) FUSION RRF WITH (rrf_k = 10, rrf_weights = [0.7, 0.3])"""))

    # --- Update ---
    stmts.append(("update-payload",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{'status': 'reviewed', 'care_path': 'stroke-alert'}} WHERE id = '{IDS['stroke']}'"))
    stmts.append(("update-filter",
        f"UPDATE {COLLECTION} SET PAYLOAD = {{'status': 'archived'}} WHERE status = 'discharged'"))

    # --- Select / Scroll ---
    stmts.append(("select-by-id",
        f"SELECT * FROM {COLLECTION} WHERE id = '{IDS['stemi']}'"))
    stmts.append(("scroll-all",
        f"SCROLL FROM {COLLECTION} LIMIT 3"))
    stmts.append(("scroll-filtered",
        f"SCROLL FROM {COLLECTION} WHERE priority = 'high' LIMIT 3"))

    # --- ORDER BY — pagination without similarity score ---
    stmts.append(("order-by-year",
        f"QUERY ORDER BY year DESC FROM {COLLECTION} LIMIT 5"))
    stmts.append(("order-by-score",
        f"QUERY ORDER BY score ASC FROM {COLLECTION} LIMIT 5 WHERE priority = 'high'"))

    # --- WITH PAYLOAD / WITH VECTORS — field selection ---
    stmts.append(("payload-false",
        f"QUERY 'acute stroke' FROM {COLLECTION} LIMIT 3 USING HYBRID WITH PAYLOAD false"))
    stmts.append(("payload-include",
        f"QUERY 'emergency' FROM {COLLECTION} LIMIT 3 USING HYBRID WITH PAYLOAD (include = ['diagnosis', 'specialty'])"))

    # --- SAMPLE — random point sampling ---
    stmts.append(("sample-random",
        f"QUERY SAMPLE FROM {COLLECTION} LIMIT 5"))

    # --- BOOST — score boosting with formula ---
    stmts.append(("boost-arithmetic",
        f"QUERY 'emergency critical' FROM {COLLECTION} LIMIT 5 USING DENSE BOOST (score * 0.3)"))
    stmts.append(("boost-conditional",
        f"QUERY 'patient treatment' FROM {COLLECTION} LIMIT 5 USING DENSE BOOST (CASE WHEN priority = 'high' THEN 2.0 ELSE 1.0 END)"))
    stmts.append(("boost-with-defaults",
        f"QUERY 'neurological' FROM {COLLECTION} LIMIT 5 USING DENSE BOOST (score * 0.5) DEFAULTS (score = 1.0)"))

    # --- Delete ---
    stmts.append(("delete-by-filter",
        f"DELETE FROM {COLLECTION} WHERE status = 'archived'"))

    # --- Show ---
    stmts.append(("show-collections", "SHOW COLLECTIONS"))
    stmts.append(("show-collection", f"SHOW COLLECTION {COLLECTION}"))

    return stmts


def main() -> None:
    parser = argparse.ArgumentParser(description="QQL Kitchen Sink — full E2E showcase")
    parser.add_argument("--execute", action="store_true", help="Run against Qdrant")
    parser.add_argument("--keep", action="store_true", help="Keep collection after run")
    parser.add_argument("--rerank", action="store_true", help="Include rerank tests (cloud only)")
    args = parser.parse_args()

    statements = build_statements()

    if args.rerank:
        statements.insert(0, ("create-collection",
            f"CREATE COLLECTION {COLLECTION} HYBRID RERANK WITH HNSW (payload_m = 16) WITH QUANTIZATION (type = 'turbo', bits = 2, always_ram = true)"))
        statements.insert(len(statements) - 2, ("search-hybrid-rerank",
            f"QUERY 'emergency neurological' FROM {COLLECTION} LIMIT 3 USING HYBRID RERANK"))
        statements.insert(len(statements) - 2, ("search-sparse-rerank",
            f"QUERY 'chest pain' FROM {COLLECTION} LIMIT 3 USING SPARSE RERANK"))

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

