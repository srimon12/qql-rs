#!/usr/bin/env python3
"""Demo: Multivector (ColBERT) and PDF Retrieval with QQL.

Showcases:
- Creating collections with multivector config (ColBERT/ColPali)
- HNSW m=0 to disable indexing for reranking vectors
- Upserting with named dense + multivector vectors
- Two-stage retrieval: prefetch with mean-pooled, rerank with original
- Prefetch with different USING vectors
- Convert command for REST JSON → QQL

Usage:
    python3 demo_multivector.py              # Print QQL statements
    python3 demo_multivector.py --execute    # Run against Qdrant
"""
from __future__ import annotations

import argparse
from _qql_cli import drop_collection_if_exists, execute_json, print_result

COLLECTION = "pdf_retrieval"


def build_statements():
    stmts = []

    # ── 1. Create collection with 3 multivector vectors ─────────
    stmts.append(("create-collection", f"""CREATE COLLECTION {COLLECTION} (
    original VECTOR(3, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0),
    mean_pooling_columns VECTOR(3, COSINE) WITH MULTIVECTOR (comparator = 'max_sim'),
    mean_pooling_rows VECTOR(3, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
)"""))

    # ── 2. Create payload indexes ───────────────────────────────
    stmts.append(("index-title",
        f"CREATE INDEX ON COLLECTION {COLLECTION} FOR title TYPE text WITH (tokenizer = 'word', min_token_len = 2, lowercase = true)"))
    stmts.append(("index-page",
        f"CREATE INDEX ON COLLECTION {COLLECTION} FOR page_number TYPE integer"))

    # ── 3. Upsert with named multivector vectors ────────────────
    pages = [
        (1, "Introduction to Vector Databases", 1,
         [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9]],
         [[0.1, 0.2, 0.3]],
         [[0.4, 0.5, 0.6]]),
        (2, "ColBERT Late Interaction Models", 2,
         [[0.2, 0.3, 0.4], [0.5, 0.6, 0.7], [0.8, 0.9, 0.1]],
         [[0.2, 0.3, 0.4]],
         [[0.5, 0.6, 0.7]]),
        (3, "Scaling PDF Retrieval with Qdrant", 3,
         [[0.3, 0.4, 0.5], [0.6, 0.7, 0.8], [0.9, 0.1, 0.2]],
         [[0.3, 0.4, 0.5]],
         [[0.6, 0.7, 0.8]]),
        (4, "Mean Pooling for Vector Compression", 4,
         [[0.4, 0.5, 0.6], [0.7, 0.8, 0.9], [0.1, 0.2, 0.3]],
         [[0.4, 0.5, 0.6]],
         [[0.7, 0.8, 0.9]]),
        (5, "HNSW Index Optimization", 5,
         [[0.5, 0.6, 0.7], [0.8, 0.9, 0.1], [0.2, 0.3, 0.4]],
         [[0.5, 0.6, 0.7]],
         [[0.8, 0.9, 0.1]]),
    ]

    for pid, title, page, orig, col_pool, row_pool in pages:
        stmts.append((f"upsert-page-{pid}", f"""UPSERT INTO {COLLECTION} VALUES {{
    id: {pid},
    title: '{title}',
    page_number: {page},
    vector: {{
        original: {orig},
        mean_pooling_columns: {col_pool},
        mean_pooling_rows: {row_pool}
    }}
}}"""))

    # ── 4. Two-stage retrieval: prefetch + rerank ───────────────
    stmts.append(("pdf-retrieval-prefetch", f"""WITH
    _pf0 AS (QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING mean_pooling_columns LIMIT 100),
    _pf1 AS (QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING mean_pooling_rows LIMIT 100)
QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING original PREFETCH (_pf0, _pf1) LIMIT 5"""))

    # ── 5. Single prefetch with filter ──────────────────────────
    stmts.append(("pdf-retrieval-filtered", f"""WITH
    _pf0 AS (QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING mean_pooling_columns WHERE page_number >= 2 LIMIT 50)
QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING original PREFETCH (_pf0) LIMIT 3"""))

    # ── 6. Dense search on named vector ─────────────────────────
    stmts.append(("search-columns-only",
        f"QUERY VECTOR [0.1, 0.2, 0.3] FROM {COLLECTION} USING mean_pooling_columns LIMIT 3"))

    # ── 7. Show collection ──────────────────────────────────────
    stmts.append(("show-collection", f"SHOW COLLECTION {COLLECTION}"))

    return stmts


def main() -> None:
    parser = argparse.ArgumentParser(description="QQL Multivector / PDF Retrieval Demo")
    parser.add_argument("--execute", action="store_true", help="Run against Qdrant")
    parser.add_argument("--keep", action="store_true", help="Keep collection after run")
    args = parser.parse_args()

    statements = build_statements()

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
