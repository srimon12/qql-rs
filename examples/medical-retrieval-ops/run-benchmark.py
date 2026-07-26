# /// script
# requires-python = ">=3.11"
# ///

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path


COLLECTION = os.environ.get("MEDICAL_RAG_COLLECTION", "medical_retrieval_ops")
DEFAULT_PATH = Path(__file__).resolve().parent / "generated" / "benchmark-questions.json"
MODES = {
    "dense": lambda q, limit: f"QUERY '{escape_qql(q)}' FROM {COLLECTION} USING dense LIMIT {limit}",
    "sparse": lambda q, limit: f"QUERY '{escape_qql(q)}' FROM {COLLECTION} USING sparse LIMIT {limit}",
    "hybrid_rrf": lambda q, limit: f"QUERY HYBRID TEXT '{escape_qql(q)}' DENSE dense SPARSE sparse FUSION RRF FROM {COLLECTION} LIMIT {limit}",
    "hybrid_dbsf": lambda q, limit: f"QUERY HYBRID TEXT '{escape_qql(q)}' DENSE dense SPARSE sparse FUSION DBSF FROM {COLLECTION} LIMIT {limit}",
    "exact": lambda q, limit: f"QUERY '{escape_qql(q)}' FROM {COLLECTION} USING dense PARAMS (exact = true) LIMIT {limit}",
}


def escape_qql(value: str) -> str:
    return value.replace("\\", "\\\\").replace("'", "\\'")


QQL_BIN = os.environ.get("QQL_BIN", "/data/codebases/qql-rs/target/debug/qql")


def run_statement(statement: str) -> dict[str, object]:
    raw = subprocess.check_output([QQL_BIN, "exec", "--quiet", "--json", statement], text=True)
    payload = json.loads(raw)
    if not payload.get("ok"):
        raise SystemExit(raw)
    return payload


def score_mode(items: list[dict[str, object]], mode_name: str) -> dict[str, object]:
    hit_at_1 = 0
    hit_at_5 = 0
    results = []

    for idx, item in enumerate(items, start=1):
        question = escape_qql(str(item["question"]))
        limit = int(item.get("limit", 5))
        statement = MODES[mode_name](question, limit)
        payload = run_statement(statement)
        result_ids = [entry["id"] for entry in payload.get("data", [])]
        expected_id = str(item["id"])
        top1 = bool(result_ids and result_ids[0] == expected_id)
        top5 = expected_id in result_ids
        hit_at_1 += int(top1)
        hit_at_5 += int(top5)
        results.append(
            {
                "index": idx,
                "id": expected_id,
                "specialty": item.get("specialty"),
                "top1_hit": top1,
                "top5_hit": top5,
                "result_ids": result_ids,
            }
        )

    total = len(items)
    return {
        "mode": mode_name,
        "queries": total,
        "hit_at_1": hit_at_1,
        "hit_at_5": hit_at_5,
        "hit_at_1_rate": round(hit_at_1 / total, 4) if total else 0.0,
        "hit_at_5_rate": round(hit_at_5 / total, 4) if total else 0.0,
        "results": results,
    }


def main() -> None:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PATH
    if not path.exists():
        raise SystemExit(f"Benchmark file not found: {path}")

    items = json.loads(path.read_text(encoding="utf-8"))
    mode_summaries = [score_mode(items, mode_name) for mode_name in MODES]
    print(
        json.dumps(
            {
                "benchmark_path": str(path),
                "collection": COLLECTION,
                "dataset_size": len(items),
                "modes": mode_summaries,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
