# /// script
# requires-python = ">=3.11"
# dependencies = ["datasets>=4.4.0"]
# ///

from __future__ import annotations

import json, os, re, hashlib
from pathlib import Path

from datasets import load_dataset

DATASET_ID = os.environ.get("MEDICAL_RAG_DATASET", "ChatMED-Project/RAGCare-QA")
MAX_ROWS_RAW = os.environ.get("MEDICAL_RAG_MAX_ROWS", "all").strip().lower()
CHUNK_SIZE = int(os.environ.get("MEDICAL_RAG_CHUNK_SIZE", "200"))
OUT_DIR = Path(os.environ.get("MEDICAL_RAG_GENERATED_DIR", Path(__file__).resolve().parent / "generated"))
CACHE_DIR = Path(os.environ.get("MEDICAL_RAG_CACHE_DIR", Path(__file__).resolve().parent / ".dataset_cache"))

SEED_PATH = OUT_DIR / "02-seed.qql"
EVAL_PATH = OUT_DIR / "eval.json"
BENCHMARK_PATH = OUT_DIR / "benchmark-questions.json"
DATASET_CACHE = CACHE_DIR / "dataset.arrow"

COLLECTION = "medical_retrieval_ops"
SPECIALTY_TENANTS = {
    "cardiology": "hospital-heart", "vascular medicine": "hospital-heart",
    "neurology": "hospital-neuro", "psychiatry": "hospital-neuro",
    "emergency medicine": "hospital-emergency", "critical care": "hospital-emergency",
}


def canonical_whitespace(value: str | None) -> str:
    return re.sub(r"\s+", " ", (value or "")).strip()


def escape_qql(value: str) -> str:
    return value.replace("\\", "\\\\").replace("'", "\\'")


def parse_max_rows() -> int | None:
    if MAX_ROWS_RAW in {"all", "full", "*"}:
        return None
    return int(MAX_ROWS_RAW)


def tenant_for_specialty(specialty: str) -> str:
    return SPECIALTY_TENANTS.get(specialty.lower(), "hospital-general")


def normalize_row(raw: dict[str, object], point_id: int) -> dict[str, str | int]:
    specialty = canonical_whitespace(str(raw.get("Type") or "general medicine"))
    question = canonical_whitespace(str(raw.get("Question") or ""))
    answer = canonical_whitespace(str(raw.get("Text Answer") or ""))
    context = canonical_whitespace(str(raw.get("Context") or ""))
    complexity = canonical_whitespace(str(raw.get("Complexity") or "intermediate"))
    ref = canonical_whitespace(str(raw.get("Reference") or ""))

    if not question or not answer or not context:
        raise ValueError("benchmark row is missing question, answer, or context")

    text = canonical_whitespace(f"Context: {context}\nSupporting answer: {answer}")
    tenant_id = tenant_for_specialty(specialty)
    priority = "high" if complexity.lower() in {"advanced", "expert"} else "medium"
    status = "active" if point_id % 7 != 0 else "review"

    return {
        "id": point_id, "tenant_id": tenant_id, "specialty": specialty,
        "complexity": complexity, "case_priority": priority, "case_status": status,
        "reference": ref, "question": question, "text_answer": answer,
        "context": context, "text": text,
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
        statements.append(f"UPSERT INTO {COLLECTION} VALUES\n  {docs}")
    statements.append(f"SHOW COLLECTION {COLLECTION}")
    SEED_PATH.write_text("\n\n".join(statements) + "\n", encoding="utf-8")


def write_benchmark(rows: list[dict[str, str | int]]) -> None:
    items = [
        {"id": row["id"], "question": row["question"], "specialty": row["specialty"], "limit": 5}
        for row in rows
    ]
    BENCHMARK_PATH.write_text(json.dumps(items, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    # Cache the dataset locally so we don't re-download every run
    cache_key = hashlib.sha256(f"{DATASET_ID}:{MAX_ROWS_RAW}".encode()).hexdigest()[:12]
    cache_file = CACHE_DIR / f"{cache_key}.json"

    if cache_file.exists():
        print(f"Loading cached dataset from {cache_file}", file=sys.stderr)
        rows_data = json.loads(cache_file.read_text())
        rows = [{k: v if k == "id" else str(v) for k, v in r.items()} for r in rows_data]
    else:
        print(f"Downloading dataset {DATASET_ID}...", file=sys.stderr)
        dataset = load_dataset(DATASET_ID, split="train")
        max_rows = parse_max_rows()

        rows: list[dict[str, str | int]] = []
        for idx, raw in enumerate(dataset, start=1):
            if max_rows is not None and len(rows) >= max_rows:
                break
            rows.append(normalize_row(raw, idx))

        if not rows:
            raise SystemExit("No usable dataset rows were found")

        # Save to cache
        cache_file.write_text(json.dumps(rows, indent=2))

    write_seed(rows)
    write_benchmark(rows)

    main_row = next((r for r in rows if r["case_status"] == "active" and r["case_priority"] == "high"), rows[0])
    related_row = next((r for r in rows if r["id"] != main_row["id"] and r["specialty"] == main_row["specialty"]), rows[1] if len(rows) > 1 else rows[0])

    manifest = {
        "dataset": DATASET_ID, "collection": COLLECTION, "row_count": len(rows),
        "chunk_size": CHUNK_SIZE, "benchmark_path": BENCHMARK_PATH.name,
        "queries": {
            "main": {
                "id": main_row["id"], "question": main_row["question"],
                "specialty": main_row["specialty"], "tenant_id": main_row["tenant_id"],
                "case_priority": main_row["case_priority"], "case_status": main_row["case_status"],
                "answer": main_row["text_answer"],
            },
            "related": {
                "id": related_row["id"], "question": related_row["question"],
                "specialty": related_row["specialty"], "tenant_id": related_row["tenant_id"],
                "answer": related_row["text_answer"],
            },
        },
    }
    EVAL_PATH.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"seed_path": SEED_PATH.name, "eval_path": EVAL_PATH.name,
                       "benchmark_path": BENCHMARK_PATH.name, "rows": len(rows)}))


if __name__ == "__main__":
    import sys
    main()
