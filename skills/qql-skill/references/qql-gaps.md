# QQL Gaps

Use this file when a request sounds reasonable in Qdrant terms but is still outside the current QQL surface.

## Not Supported Yet

- local/external rerank (`RERANK` is cloud-only)
- offset-style pagination for grouped search
- MMR for `USING SPARSE` or `RECOMMEND`
- custom vector on-disk toggles
- ReadConsistency / ShardKeySelector / Timeout controls

## What To Say

Prefer plain language:

- `QQL does not support this yet.`
- `This needs raw Qdrant SDK usage or a QQL extension.`
- `The closest supported QQL form is ...`

## Practical Fallbacks

- Need exact baseline: use `EXACT`
- Need single point by exact ID: use `SELECT * FROM <collection> WHERE id = ...`
- Need to browse or export points page by page: use `SCROLL FROM <collection> ... LIMIT <n>`
- Need pagination without similarity score: use `QUERY ORDER BY <field> [ASC|DESC] FROM <collection> LIMIT <n>`
- Need to filter returned fields: use `WITH PAYLOAD (include=['f1'], exclude=['f2']) WITH VECTOR ('name')`
- Need recall tuning: use `WITH (hnsw_ef = ...)`
- Need flat search pagination: use `QUERY ... LIMIT <n> OFFSET <n>`
- Need low-score filtering: use `QUERY ... SCORE THRESHOLD <float|int>`
- Need cross-collection lookup: use `QUERY ... LOOKUP FROM <collection> [VECTOR '<name>']`
- Need keyword plus semantic retrieval: use `USING HYBRID`
- Need parameterized RRF tuning: use `WITH (rrf_k = <n>, rrf_weights = [...])`
- Need multi-stage retrieval with per-prefetch filters: use `WITH <name> AS (...) ... PREFETCH (name WHERE <filter> SCORE THRESHOLD <n>) FUSION RRF`
- Need hybrid DBSF fusion: use `USING HYBRID FUSION DBSF`
- Need better ordering: use `RERANK` (cloud only)
- Need filtering: create an index first, then use `WHERE`
- Need grouped top results by field: use `QUERY ... GROUP BY <field> [GROUP_SIZE <n>]`
- Need cross-collection group lookup: use `QUERY ... GROUP BY <field> GROUP_SIZE <n> WITH LOOKUP FROM <collection>`
- Need to patch metadata in place: use `UPDATE <collection> SET PAYLOAD = {...} WHERE ...`
- Need to replace a stored vector: use `UPDATE <collection> SET VECTOR = [...] WHERE id = ...`
- Need a runnable prototype: stay inside `CREATE`, `CREATE INDEX`, `UPSERT`, `QUERY`, `DELETE`
- Need batch upsert: use comma-separated `UPSERT INTO <name> VALUES {...}, {...}`
- Need script round-trip: use `qql-go execute` and `qql-go dump [--batch-size N]`
- Need local inference without cloud: use `qql-go connect --inference-mode local`
- Need score boosting: use `BOOST ($score + 0.3 * popularity)` or `BOOST (CASE WHEN ... THEN ... ELSE ... END)`
- Need random sampling: use `QUERY SAMPLE FROM <collection> LIMIT <n>`
- Need geo-distance decay: use `BOOST ($score * GAUSS_DECAY(GEO_DISTANCE(lat, lon, field), 0, 5000, 0.5))`
- Need conditional scoring: use `BOOST (CASE WHEN <filter> THEN <expr> ELSE <expr> END)`
- Need mathematical score shaping: use `BOOST (SQRT($score) * LOG(citations + 1))`

## Reminder

Do not hide missing features behind made-up syntax. If the current CLI cannot parse and execute it, it is outside this skill.
