# /// script
# requires-python = ">=3.11"
# dependencies = ["datasets>=4.4.0"]
# ///

"""Build the medical-retrieval-ops demo corpus from the public RAGCare-QA benchmark.

The source dataset is a public synthetic benchmark (not real PHI), but rows
still contain medical Q&A text. Generated files stay local under
``generated/`` / ``.dataset_cache/`` with owner-only (0600) permissions, and
stdout/stderr logs carry only file names and row counts — never question,
answer, or context text.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
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


def write_restricted(path: Path, text: str) -> None:
    """Write demo output with owner-only permissions (no PHI in git)."""
    # codeql[py/clear-text-storage-sensitive-data] False positive: the content
    # is public benchmark text (RAGCare-QA). Files are local-only, git-ignored,
    # and written owner-only (0600); clear-text output is the tool's purpose.
    path.write_text(text, encoding="utf-8")
    try:
        os.chmod(path, 0o600)
    except OSError:
        pass


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
    write_restricted(SEED_PATH, "\n\n".join(statements) + "\n")


def write_benchmark(rows: list[dict[str, str | int]]) -> None:
    items = [
        {"id": row["id"], "question": row["question"], "specialty": row["specialty"], "limit": 5}
        for row in rows
    ]
    write_restricted(BENCHMARK_PATH, json.dumps(items, indent=2) + "\n")


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    # Cache the dataset locally so we don't re-download every run
    cache_key = hashlib.sha256(f"{DATASET_ID}:{MAX_ROWS_RAW}".encode()).hexdigest()[:12]
    cache_file = CACHE_DIR / f"{cache_key}.json"

    if cache_file.exists():
        # Log only the cache path, never row contents.
        print(f"Loading cached dataset from {cache_file}", file=sys.stderr)
        rows_data = json.loads(cache_file.read_text(encoding="utf-8"))
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

        # Local cache only (0600); never log row contents.
        write_restricted(cache_file, json.dumps(rows, indent=2) + "\n")

    write_seed(rows)
    write_benchmark(rows)

    main_row = next((r for r in rows if r["case_status"] == "active" and r["case_priority"] == "high"), rows[0])
    related_row = next((r for r in rows if r["id"] != main_row["id"] and r["specialty"] == main_row["specialty"]), rows[1] if len(rows) > 1 else rows[0])

    # Eval manifest keeps only the fields the demo/benchmark actually need
    # (ids, questions, routing labels). Full answers/context stay in the
    # restricted seed file; they are never printed to stdout/stderr.
    manifest = {
        "dataset": DATASET_ID, "collection": COLLECTION, "row_count": len(rows),
        "chunk_size": CHUNK_SIZE, "benchmark_path": BENCHMARK_PATH.name,
        "queries": {
            "main": {
                "id": main_row["id"], "question": main_row["question"],
                "specialty": main_row["specialty"], "tenant_id": main_row["tenant_id"],
                "case_priority": main_row["case_priority"], "case_status": main_row["case_status"],
            },
            "related": {
                "id": related_row["id"], "question": related_row["question"],
                "specialty": related_row["specialty"], "tenant_id": related_row["tenant_id"],
            },
        },
    }
    write_restricted(EVAL_PATH, json.dumps(manifest, indent=2) + "\n")
    # Machine-readable summary: file names + counts only, no Q&A text.
    print(json.dumps({"seed_path": SEED_PATH.name, "eval_path": EVAL_PATH.name,
                       "benchmark_path": BENCHMARK_PATH.name, "rows": len(rows)}))


if __name__ == "__main__":
    main()
