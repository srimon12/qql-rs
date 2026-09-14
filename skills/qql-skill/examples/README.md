# QQL examples

Pure QQL files grouped by task. Each file parses with current QQL. Copy the block you need. Language rules live in `../references/`. SDK wiring lives in the per-SDK references.

| File | Covers |
|------|--------|
| `hybrid-fusion.qql` | Multi-stage hybrid, tiered CTEs, per-prefetch filters, RAG grouping, hybrid plus formula |
| `multivector-rerank.qql` | ColBERT nearest, late-interaction rerank, cross-encoder rerank |
| `recommend-discover.qql` | Recommend, context, discover, relevance feedback |
| `filters.qql` | Complex filter chains, at-least-N, token and except match |
| `formula.qql` | Formula boost, conditional scoring, geo decay, math shaping, recency decay |
| `grouping.qql` | Grouped retrieval, cross-collection lookup, prefetch routing |
| `ordering-paging.qql` | Order by browse, scroll ordering, start-from resume |
| `projections.qql` | Payload and vector projections, compact literals |
| `search-params.qql` | MMR, geo filters, quantization params, ACORN, timeout and consistency |
| `lifecycle.qql` | Full setup script, shard lifecycle, batch scripts |
| `admin.qql` | Quotas, memory tiers, prefix index, slice sampling, IDF corpus, facet, conditional upsert, inference shapes, collection blocks |
