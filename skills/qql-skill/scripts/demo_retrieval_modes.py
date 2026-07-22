#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json


EXAMPLES = [
    {
        "mode": "dense",
        "when": "Use when semantic similarity matters more than exact term matching.",
        "query": "QUERY 'vector database performance tuning' FROM articles USING dense LIMIT 5",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "dense-by-id",
        "when": "Use when you want to find results similar to a specific point by its ID.",
        "query": "QUERY POINTS ('123e4567-e89b-12d3-a456-426614174001') FROM articles WITH PAYLOAD true",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid",
        "when": "Use when exact terms, acronyms, model names, or error strings matter.",
        "query": (
            "QUERY HYBRID TEXT 'out of memory hnsw_ef acorn' DENSE dense SPARSE sparse FUSION RRF FROM incidents "
            "LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-dbsf",
        "when": "Use when you want hybrid retrieval with DBSF fusion instead of the default RRF.",
        "query": (
            "QUERY HYBRID TEXT 'out of memory hnsw_ef acorn' DENSE dense SPARSE sparse FUSION DBSF FROM incidents "
            "LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-rrf-params",
        "when": "Use when you want to tune RRF parameters — K controls rank smoothing, weights control source influence.",
        "query": (
            "QUERY HYBRID TEXT 'vector search performance' DENSE dense SPARSE sparse FUSION RRF FROM articles "
            "WITH (rrf_k = 30, rrf_weights = [0.7, 0.3]) LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "sparse",
        "when": "Use when keyword or BM25 retrieval matters more than semantic similarity.",
        "query": (
            "QUERY 'out of memory hnsw_ef acorn' FROM incidents "
            "USING sparse LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "exact",
        "when": "Use when debugging recall and you need an exact KNN baseline.",
        "query": "QUERY 'attention mechanism' FROM articles USING dense PARAMS (exact = true) LIMIT 10",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "with-hnsw-ef",
        "when": "Use when you want query-time recall tuning without changing collection config.",
        "query": (
            "QUERY 'transformer inference' FROM articles "
            "USING dense PARAMS (hnsw_ef = 256) LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "score-threshold",
        "when": "Use when you want to filter out low-relevance results at query time.",
        "query": (
            "QUERY 'vector database' FROM articles "
            "USING dense SCORE THRESHOLD 0.5 LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "offset",
        "when": "Use when you need to paginate through flat search results.",
        "query": (
            "QUERY 'vector database' FROM articles "
            "USING dense LIMIT 5 OFFSET 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-mmr",
        "when": "Use when hybrid search results are too redundant and you want semantic diversity on the dense leg before fusion.",
        "query": (
            "QUERY MMR TEXT 'vector database performance tuning' DIVERSITY 0.5 CANDIDATES 25 FROM articles "
            "USING dense LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "with-filter",
        "when": "Use when metadata constraints should narrow the search. Requires CREATE INDEX first.",
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR category TYPE keyword",
        ],
        "query": (
            "QUERY 'transformer inference' FROM articles "
            "USING dense WHERE category = 'ml' LIMIT 10"
        ),
        "requires_index": ["category"],
    },
    {
        "mode": "with-acorn",
        "when": "Use when filtered-query recall is the focus and ACORN should be tested.",
        "query": (
            "QUERY 'retrieval recall regression' FROM incidents "
            "USING dense WHERE team = 'search' PARAMS (acorn = true) LIMIT 10"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION incidents FOR team TYPE keyword",
        ],
        "requires_index": ["team"],
    },
    {
        "mode": "tenant-aware-indexing",
        "when": "Use when a filter field acts like a tenant boundary and Qdrant should optimize for that grouping.",
        "query": (
            "QUERY 'stroke discharge summary' FROM tenant_docs "
            "USING dense WHERE tenant_id = 'tenant-a' LIMIT 5"
        ),
        "setup": [
            "CREATE COLLECTION tenant_docs (dense VECTOR (384, COSINE), sparse SPARSE) HYBRID WITH HNSW (payload_m = 16)",
            "CREATE INDEX ON COLLECTION tenant_docs FOR tenant_id TYPE keyword WITH (is_tenant = true, on_disk = true)",
        ],
        "requires_index": ["tenant_id"],
    },
    {
        "mode": "text-index-tuning",
        "when": "Use when a text payload field needs explicit tokenization controls before phrase or keyword-heavy filtering.",
        "query": (
            "CREATE INDEX ON COLLECTION tenant_docs FOR title TYPE text "
            "WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true, phrase_matching = true)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "grouped",
        "when": "Use when results should be grouped by a payload field instead of returned as one flat list.",
        "query": (
            "QUERY 'retrieval recall regression' FROM incidents "
            "USING dense GROUP BY team SIZE 2 LIMIT 5"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION incidents FOR team TYPE keyword",
        ],
        "requires_index": ["team"],
    },
    {
        "mode": "grouped-hybrid",
        "when": "Use when grouped results still need hybrid recall and query-time tuning.",
        "query": (
            "QUERY 'retrieval recall regression' FROM incidents "
            "USING dense PARAMS (hnsw_ef = 128) GROUP BY team SIZE 2 LIMIT 4"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION incidents FOR team TYPE keyword",
        ],
        "requires_index": ["team"],
    },
    {
        "mode": "recommend",
        "when": "Use when you have example point IDs and want to find similar items.",
        "query": (
            "QUERY RECOMMEND POSITIVE ('uuid-1', 'uuid-2') FROM articles USING dense LIMIT 5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "recommend-with-strategy",
        "when": "Use when you want to control how positive/negative examples are combined.",
        "query": (
            "QUERY RECOMMEND POSITIVE ('uuid-1') NEGATIVE ('uuid-2') "
            "STRATEGY best_score FROM articles USING dense LIMIT 5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "context",
        "when": "Use when you have pairwise relevance signals (this is better than that) and want context-aware search.",
        "query": (
            "QUERY CONTEXT (POSITIVE POINT 'uuid-1' NEGATIVE POINT 'uuid-2', POSITIVE POINT 'uuid-3' NEGATIVE POINT 'uuid-4') FROM docs USING dense LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "discover",
        "when": "Use when you have a target item and context pairs to explore an interesting region of the vector space.",
        "query": (
            "QUERY DISCOVER TARGET POINT 'uuid-1' CONTEXT (POSITIVE POINT 'uuid-2' NEGATIVE POINT 'uuid-3') FROM docs USING dense LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf",
        "when": "Use when you need multi-stage retrieval with separate dense and sparse prefetch legs combined via RRF.",
        "query": (
            "WITH a AS (QUERY 'search query' FROM docs USING dense LIMIT 100),\n"
            "     b AS (QUERY 'search query' FROM docs USING sparse LIMIT 100)\n"
            "QUERY FUSION RRF FROM docs PREFETCH (a, b) LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf-per-filter",
        "when": "Use when each prefetch leg needs its own filter and score threshold. Per-prefetch WHERE and SCORE THRESHOLD are pushed down to Qdrant — not post-filters.",
        "query": (
            "WITH a AS (QUERY 'search query' FROM docs USING dense WHERE category = 'tech' LIMIT 200),\n"
            "     b AS (QUERY 'search query' FROM docs USING sparse LIMIT 300)\n"
            "QUERY FUSION RRF FROM docs PREFETCH (a SCORE THRESHOLD 0.6, b SCORE THRESHOLD 0.3)\n"
            "  WITH (rrf_k = 20, rrf_weights = [0.6, 0.4]) LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf-tiered",
        "when": "Use when you want a broad first pass scoped by a narrower second pass — coarse-to-fine retrieval for RAG pipelines.",
        "query": (
            "WITH broad AS (QUERY 'emergency neurological' FROM clinical_docs USING dense WHERE department = 'emergency' LIMIT 500),\n"
            "     narrow AS (QUERY 'emergency neurological' FROM clinical_docs USING sparse PREFETCH (broad) LIMIT 100)\n"
            "QUERY FUSION RRF FROM clinical_docs PREFETCH (narrow) LIMIT 5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "grouped-with-lookup",
        "when": "Use when you search in one collection but group IDs live in a separate collection. WITH LOOKUP FROM resolves group IDs from the lookup collection.",
        "query": (
            "QUERY 'machine learning' FROM research_papers USING dense\n"
            "  GROUP BY author_id SIZE 5\n"
            "  LOOKUP FROM author_metadata WITH PAYLOAD true LIMIT 20"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "rerank",
        "when": "Use when recall is likely good but top-result ordering needs help. Requires Qdrant Cloud and a rerank-capable collection.",
        "query": (
            "QUERY 'late interaction retrieval' FROM papers USING dense LIMIT 5 RERANK"
        ),
        "setup": [],
        "requires_index": [],
        "requires_cloud": True,
    },
    {
        "mode": "hybrid-rerank",
        "when": "Use when both keyword recall and top-rank precision matter. Requires Qdrant Cloud and a rerank-capable collection.",
        "query": (
            "QUERY HYBRID TEXT 'cross encoder ms marco minimlm' DENSE dense SPARSE sparse FUSION RRF FROM docs "
            "LIMIT 8 RERANK"
        ),
        "setup": [],
        "requires_index": [],
        "requires_cloud": True,
    },
    {
        "mode": "sparse-rerank",
        "when": "Use when sparse recall is strong but the top ordering still needs rerank. Requires Qdrant Cloud and a rerank-capable collection.",
        "query": (
            "QUERY 'cross encoder ms marco minimlm' FROM docs "
            "USING sparse LIMIT 8 RERANK"
        ),
        "setup": [],
        "requires_index": [],
        "requires_cloud": True,
    },
    {
        "mode": "sample-random",
        "when": "Use when you need random point sampling for exploration, testing, or dashboards.",
        "query": "QUERY SAMPLE RANDOM FROM articles LIMIT 10",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "order-by-field",
        "when": "Use when you need paginated results ordered by a payload field instead of similarity score.",
        "query": "QUERY ORDER BY created_at DESC FROM articles LIMIT 20",
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR created_at TYPE integer",
        ],
        "requires_index": ["created_at"],
    },
    {
        "mode": "select-by-id",
        "when": "Use when you already know the exact point ID and want the stored payload.",
        "query": "QUERY POINTS ('pt-42') FROM articles WITH PAYLOAD true",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "scroll",
        "when": "Use when you need to page through a collection or walk filtered points.",
        "query": (
            "SCROLL FROM articles WHERE category = 'ml' AFTER 'pt-42' LIMIT 25"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR category TYPE keyword",
        ],
        "requires_index": ["category"],
    },
    {
        "mode": "delete-by-field",
        "when": "Delete points by field value instead of ID.",
        "query": ("DELETE FROM articles WHERE category = 'archived'"),
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR category TYPE keyword",
        ],
        "requires_index": ["category"],
    },
    {
        "mode": "update-payload",
        "when": "Patch stored metadata in place for one point or a filtered subset.",
        "query": (
            "UPDATE articles SET PAYLOAD = {status: 'published'} WHERE category = 'draft'"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR category TYPE keyword",
        ],
        "requires_index": ["category"],
    },
    {
        "mode": "update-vector",
        "when": "Replace the stored dense vector for one exact point ID.",
        "query": "UPDATE articles SET VECTOR dense = [0.1, 0.2, 0.3] WHERE id = 42",
        "setup": [],
        "requires_index": [],
    },
]


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Print compact QQL retrieval examples."
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit the examples as JSON.",
    )
    args = parser.parse_args()

    if args.json:
        print(json.dumps(EXAMPLES, indent=2))
        return

    for example in EXAMPLES:
        print(f"[{example['mode']}]")
        print(example["when"])
        for setup_stmt in example.get("setup", []):
            print(f"  Setup: {setup_stmt}")
        if example.get("requires_index"):
            print(f"  Note: Requires index on {example['requires_index']}")
        if example.get("requires_cloud"):
            print(f"  Note: Requires Qdrant Cloud and a rerank-capable collection")
        print(example["query"])
        print()


if __name__ == "__main__":
    main()
