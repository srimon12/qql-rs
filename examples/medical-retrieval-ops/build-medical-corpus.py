# /// script
# requires-python = ">=3.11"
# dependencies = ["datasets>=4.4.0"]
# ///

from __future__ import annotations

import json
import os
import re
from pathlib import Path

from datasets import load_dataset


DATASET_ID = os.environ.get("MEDICAL_RAG_DATASET", "ChatMED-Project/RAGCare-QA")
MAX_ROWS_RAW = os.environ.get("MEDICAL_RAG_MAX_ROWS", "all").strip().lower()
CHUNK_SIZE = int(os.environ.get("MEDICAL_RAG_CHUNK_SIZE", "200"))
OUT_DIR = Path(os.environ.get("MEDICAL_RAG_GENERATED_DIR", Path(__file__).resolve().parent / "generated"))
SEED_PATH = OUT_DIR / "02-seed.qql"
EVAL_PATH = OUT_DIR / "eval.json"
BENCHMARK_PATH = OUT_DIR / "benchmark-questions.json"

COLLECTION = "medical_retrieval_ops"
SPECIALTY_TENANTS = {
    "cardiology": "hospital-heart",
    "vascular medicine": "hospital-heart",
    "neurology": "hospital-neuro",
    "psychiatry": "hospital-neuro",
    "emergency medicine": "hospital-emergency",
    "critical care": "hospital-emergency",
}


def canonical_whitespace(value: str | None) -> str:
    cleaned = re.sub(r"\s+", " ", (value or "")).strip()
    return cleaned.replace("--", "-")


def escape_qql(value: str) -> str:
    return value.replace("\\", "\\\\").replace("'", "\\'").replace("\n", "\\n")


def parse_max_rows() -> int | None:
    if MAX_ROWS_RAW in {"all", "full", "*"}:
        return None
    return int(MAX_ROWS_RAW)


def tenant_for_specialty(specialty: str) -> str:
    return SPECIALTY_TENANTS.get(specialty.lower(), "hospital-general")


def priority_for_complexity(complexity: str) -> str:
    level = complexity.lower()
    if level in {"advanced", "expert"}:
        return "high"
    if level in {"intermediate", "medium"}:
        return "medium"
    return "high" if level == "basic" else "medium"


def normalize_row(raw: dict[str, object], point_id: int) -> dict[str, str | int]:
    specialty = canonical_whitespace(str(raw.get("Type") or "general medicine"))
    question = canonical_whitespace(str(raw.get("Question") or ""))
    answer = canonical_whitespace(str(raw.get("Text Answer") or ""))
    context = canonical_whitespace(str(raw.get("Context") or ""))
    complexity = canonical_whitespace(str(raw.get("Complexity") or "unknown"))
    rag_pipeline = canonical_whitespace(str(raw.get("RAG Pipeline") or "unknown"))
    reference = canonical_whitespace(str(raw.get("Reference") or "unknown"))

    if not question or not answer or not context:
        raise ValueError("benchmark row is missing question, answer, or context")

    text = canonical_whitespace(f"Context: {context}\nSupporting answer: {answer}")
    tenant_id = tenant_for_specialty(specialty)
    case_priority = priority_for_complexity(complexity)
    case_status = "active" if point_id % 7 != 0 else "review"
    return {
        "id": point_id,
        "source_dataset": DATASET_ID,
        "tenant_id": tenant_id,
        "specialty": specialty,
        "complexity": complexity,
        "case_priority": case_priority,
        "case_status": case_status,
        "rag_pipeline": rag_pipeline,
        "reference": reference,
        "question": question,
        "text_answer": answer,
        "topic_text": question,
        "context": context,
        "text": text,
    }


def render_doc(row: dict[str, str | int]) -> str:
    parts = []
    for key, value in row.items():
        if key == "id":
            parts.append(f"'{key}': {value}")
        else:
            parts.append(f"'{key}': '{escape_qql(str(value))}'")
    return "{\n    " + ",\n    ".join(parts) + "\n  }"


def write_seed(rows: list[dict[str, str | int]]) -> None:
    statements: list[str] = []
    for idx in range(0, len(rows), CHUNK_SIZE):
        chunk = rows[idx : idx + CHUNK_SIZE]
        docs = ",\n".join(render_doc(row) for row in chunk)
        statements.append(f"UPSERT INTO {COLLECTION} VALUES\n  {docs}\nUSING HYBRID")
    statements.append(f"SHOW COLLECTION {COLLECTION}")
    SEED_PATH.write_text("\n\n".join(statements) + "\n", encoding="utf-8")


def write_benchmark(rows: list[dict[str, str | int]]) -> None:
    items = [
        {
            "id": row["id"],
            "question": row["question"],
            "specialty": row["specialty"],
            "limit": 5,
        }
        for row in rows
    ]
    BENCHMARK_PATH.write_text(json.dumps(items, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    dataset = load_dataset(DATASET_ID, split="train")
    max_rows = parse_max_rows()

    rows: list[dict[str, str | int]] = []
    for idx, raw in enumerate(dataset, start=1):
        if max_rows is not None and len(rows) >= max_rows:
            break
        rows.append(normalize_row(raw, idx))

    if not rows:
        raise SystemExit("No usable dataset rows were found")

    write_seed(rows)
    write_benchmark(rows)

    main_row = next((row for row in rows if row["case_status"] == "active" and row["case_priority"] == "high"), rows[0])
    related_row = next((row for row in rows if row["id"] != main_row["id"] and row["specialty"] == main_row["specialty"]), rows[1] if len(rows) > 1 else rows[0])
    manifest = {
        "dataset": DATASET_ID,
        "collection": COLLECTION,
        "row_count": len(rows),
        "full_dataset": max_rows is None,
        "chunk_size": CHUNK_SIZE,
        "benchmark_path": BENCHMARK_PATH.name,
        "queries": {
            "main": {
                "id": main_row["id"],
                "question": main_row["question"],
                "specialty": main_row["specialty"],
                "tenant_id": main_row["tenant_id"],
                "case_priority": main_row["case_priority"],
                "case_status": main_row["case_status"],
                "answer": main_row["text_answer"],
            },
            "related": {
                "id": related_row["id"],
                "question": related_row["question"],
                "specialty": related_row["specialty"],
                "tenant_id": related_row["tenant_id"],
                "answer": related_row["text_answer"],
            },
        },
    }
    EVAL_PATH.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "seed_path": SEED_PATH.name,
                "eval_path": EVAL_PATH.name,
                "benchmark_path": BENCHMARK_PATH.name,
                "rows": len(rows),
            }
        )
    )


if __name__ == "__main__":
    main()
