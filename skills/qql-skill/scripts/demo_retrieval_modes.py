#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json


EXAMPLES = [
    {
        "mode": "dense",
        "when": "Use when semantic similarity matters more than exact term matching.",
        "query": "QUERY 'vector database performance tuning' FROM articles LIMIT 5",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "dense-by-id",
        "when": "Use when you want to find results similar to a specific point by its ID.",
        "query": "QUERY '123e4567-e89b-12d3-a456-426614174001' FROM articles LIMIT 5",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid",
        "when": "Use when exact terms, acronyms, model names, or error strings matter.",
        "query": (
            "QUERY 'out of memory hnsw_ef acorn' FROM incidents "
            "LIMIT 10 USING HYBRID"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-dbsf",
        "when": "Use when you want hybrid retrieval with DBSF fusion instead of the default RRF.",
        "query": (
            "QUERY 'out of memory hnsw_ef acorn' FROM incidents "
            "LIMIT 10 USING HYBRID FUSION DBSF"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-rrf-params",
        "when": "Use when you want to tune RRF parameters — K controls rank smoothing, weights control source influence.",
        "query": (
            "QUERY 'vector search performance' FROM articles "
            "LIMIT 10 USING HYBRID WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "sparse",
        "when": "Use when keyword or BM25 retrieval matters more than semantic similarity.",
        "query": (
            "QUERY 'out of memory hnsw_ef acorn' FROM incidents "
            "LIMIT 10 USING SPARSE"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "exact",
        "when": "Use when debugging recall and you need an exact KNN baseline.",
        "query": "QUERY 'attention mechanism' FROM articles LIMIT 10 EXACT",
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "with-hnsw-ef",
        "when": "Use when you want query-time recall tuning without changing collection config.",
        "query": (
            "QUERY 'transformer inference' FROM articles "
            "LIMIT 10 WITH (hnsw_ef = 256)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "score-threshold",
        "when": "Use when you want to filter out low-relevance results at query time.",
        "query": (
            "QUERY 'vector database' FROM articles "
            "LIMIT 10 SCORE THRESHOLD 0.5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "offset",
        "when": "Use when you need to paginate through flat search results.",
        "query": (
            "QUERY 'vector database' FROM articles "
            "LIMIT 5 OFFSET 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "hybrid-mmr",
        "when": "Use when hybrid search results are too redundant and you want semantic diversity on the dense leg before fusion.",
        "query": (
            "QUERY 'vector database performance tuning' FROM articles "
            "LIMIT 10 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 25)"
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
            "LIMIT 10 WHERE category = 'ml'"
        ),
        "requires_index": ["category"],
    },
    {
        "mode": "with-acorn",
        "when": "Use when filtered-query recall is the focus and ACORN should be tested.",
        "query": (
            "QUERY 'retrieval recall regression' FROM incidents "
            "LIMIT 10 WHERE team = 'search' WITH (acorn = true)"
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
            "LIMIT 5 WHERE tenant_id = 'tenant-a'"
        ),
        "setup": [
            "CREATE COLLECTION tenant_docs HYBRID WITH HNSW (payload_m = 16)",
            "CREATE INDEX ON COLLECTION tenant_docs FOR tenant_id TYPE keyword WITH (is_tenant = true, on_disk = true)",
        ],
        "requires_index": ["tenant_id"],
    },
    {
        "mode": "text-index-tuning",
        "when": "Use when a text payload field needs explicit tokenization controls before phrase or keyword-heavy filtering.",
        "query": (
            "CREATE INDEX ON COLLECTION tenant_docs FOR title TYPE text "
            "WITH (tokenizer = 'word', min_token_len: 2, max_token_len: 20, lowercase: true, phrase_matching: true)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "grouped",
        "when": "Use when results should be grouped by a payload field instead of returned as one flat list.",
        "query": (
            "QUERY 'retrieval recall regression' FROM incidents "
            "LIMIT 5 GROUP BY team GROUP_SIZE 2"
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
            "LIMIT 4 USING HYBRID WITH (hnsw_ef = 128) "
            "GROUP BY team GROUP_SIZE 2"
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
            "QUERY RECOMMEND WITH (positive = ('uuid-1', 'uuid-2')) FROM articles LIMIT 5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "recommend-with-strategy",
        "when": "Use when you want to control how positive/negative examples are combined.",
        "query": (
            "QUERY RECOMMEND WITH (positive = ('uuid-1')), negative = ('uuid-2') "
            "STRATEGY 'best_score' FROM articles LIMIT 5"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "context",
        "when": "Use when you have pairwise relevance signals (this is better than that) and want context-aware search.",
        "query": (
            "QUERY CONTEXT PAIRS (('uuid-1', 'uuid-2'), ('uuid-3', 'uuid-4')) FROM docs LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "discover",
        "when": "Use when you have a target item and context pairs to explore an interesting region of the vector space.",
        "query": (
            "QUERY DISCOVER TARGET 'uuid-1' CONTEXT PAIRS (('uuid-2', 'uuid-3')) FROM docs LIMIT 10"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf",
        "when": "Use when you need multi-stage retrieval with separate dense and sparse prefetch legs combined via RRF.",
        "query": (
            "WITH a AS (QUERY 'search query' USING dense LIMIT 100),\n"
            "     b AS (QUERY 'search query' USING sparse LIMIT 100)\n"
            "QUERY 'search query' FROM docs LIMIT 10\n"
            "  PREFETCH (a, b)\n"
            "  FUSION RRF"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf-per-filter",
        "when": "Use when each prefetch leg needs its own filter and score threshold. Per-prefetch WHERE and SCORE THRESHOLD are pushed down to Qdrant — not post-filters.",
        "query": (
            "WITH a AS (QUERY 'search query' USING dense LIMIT 200 WHERE category = 'tech'),\n"
            "     b AS (QUERY 'search query' USING sparse LIMIT 300)\n"
            "QUERY 'search query' FROM docs LIMIT 10\n"
            "  PREFETCH (a SCORE THRESHOLD 0.6, b SCORE THRESHOLD 0.3)\n"
            "  FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "prefetch-rrf-tiered",
        "when": "Use when you want a broad first pass scoped by a narrower second pass — coarse-to-fine retrieval for RAG pipelines.",
        "query": (
            "WITH broad AS (QUERY 'emergency neurological' USING dense LIMIT 500 WHERE department = 'emergency'),\n"
            "     narrow AS (QUERY 'emergency neurological' USING sparse LIMIT 100 PREFETCH (broad))\n"
            "QUERY 'emergency neurological' FROM clinical_docs LIMIT 5\n"
            "  PREFETCH (narrow)\n"
            "  FUSION RRF"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "grouped-with-lookup",
        "when": "Use when you search in one collection but group IDs live in a separate collection. WITH LOOKUP FROM resolves group IDs from the lookup collection.",
        "query": (
            "QUERY 'machine learning' FROM research_papers LIMIT 20\n"
            "  GROUP BY 'author_id' GROUP_SIZE 5\n"
            "  WITH LOOKUP FROM author_metadata"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "rerank",
        "when": "Use when recall is likely good but top-result ordering needs help. Requires Qdrant Cloud and a rerank-capable collection.",
        "query": (
            "QUERY 'late interaction retrieval' FROM papers LIMIT 5 RERANK"
        ),
        "setup": [],
        "requires_index": [],
        "requires_cloud": True,
    },
    {
        "mode": "hybrid-rerank",
        "when": "Use when both keyword recall and top-rank precision matter. Requires Qdrant Cloud and a rerank-capable collection.",
        "query": (
            "QUERY 'cross encoder ms marco minimlm' FROM docs "
            "LIMIT 8 USING HYBRID RERANK"
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
            "LIMIT 8 USING SPARSE RERANK"
        ),
        "setup": [],
        "requires_index": [],
        "requires_cloud": True,
    },
    {
        "mode": "sample-random",
        "when": "Use when you need random point sampling for exploration, testing, or dashboards.",
        "query": "QUERY SAMPLE FROM articles LIMIT 10",
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
        "mode": "boost-arithmetic",
        "when": "Use when you want to modify search scores using payload fields — e.g., boost by popularity or freshness.",
        "query": (
            "QUERY 'vector database' FROM articles LIMIT 10 "
            "BOOST (score + 0.3 * popularity)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "boost-conditional",
        "when": "Use when you want different scoring logic for different categories — e.g., premium content gets 2x boost.",
        "query": (
            "QUERY 'kubernetes best practices' FROM docs LIMIT 10 "
            "BOOST (CASE WHEN category = 'premium' THEN score * 2.0 ELSE score END)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "boost-geo-decay",
        "when": "Use when you want distance-based scoring decay — closer results score higher with a smooth falloff.",
        "query": (
            "QUERY 'restaurant' FROM places LIMIT 10 "
            "BOOST (score * GAUSS_DECAY(GEO_DISTANCE(48.8566, 2.3522, location), 0, 5000, 0.5))"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "boost-math-functions",
        "when": "Use when you need non-linear score transformations — logarithmic dampening, square root for variance reduction.",
        "query": (
            "QUERY 'machine learning' FROM papers LIMIT 10 "
            "BOOST (SQRT(score) * LOG(citation_count + 1)) DEFAULTS (citation_count = 0)"
        ),
        "setup": [],
        "requires_index": [],
    },
    {
        "mode": "select-by-id",
        "when": "Use when you already know the exact point ID and want the stored payload.",
        "query": "SELECT * FROM articles WHERE id = 'pt-42'",
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
            "UPDATE articles SET PAYLOAD WHERE category = 'draft' "
            "{'status': 'published'}"
        ),
        "setup": [
            "CREATE INDEX ON COLLECTION articles FOR category TYPE keyword",
        ],
        "requires_index": ["category"],
    },
    {
        "mode": "update-vector",
        "when": "Replace the stored dense vector for one exact point ID.",
        "query": "UPDATE articles SET VECTOR WHERE id = 42 [0.1, 0.2, 0.3]",
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
