# QQL Gaps

Use this file when a request sounds reasonable in Qdrant terms but is still outside the current QQL surface.

## Not Supported Yet

- Offset-style pagination for grouped search
- MMR for `USING SPARSE` or `RECOMMEND`
- ReadConsistency / Timeout controls via QQL syntax (timeout is in the Executor config layer, not the language)
- `USING HYBRID` shorthand (use `QUERY HYBRID TEXT '...' DENSE ... SPARSE ...`)
- Dynamic shard routing key resolution (shard key must be explicitly provided)
- `max_selectivity` on `PARAMS (acorn = true)` -- the plan type has the field but it is not settable from QQL syntax yet
- Built-in ColBERT host models: `Embedder::embed_multi` is required for multivector TEXT embedding; default implementations error with `QQL-EMBEDDING-MULTI` until the host supplies a multi-vector model (HttpEmbedder / FastEmbedder do not invent ColBERT weights)
- UPSERT auto-embed into multivector slots from plain text (pass precomputed `vector: { colbert: [[...], ...] }` or extend the host)

## Vector roles — do not invent

These are **supported** (do not claim they need raw SDK work):

- `USING name` without `AS` when executing against a live collection (schema fills dense/sparse/multi)
- `USING name AS MULTI` for ColBERT-style multivector query text
- `QUERY RERANK … USING colbert` with multivector schema + host `embed_multi`
- Precomputed multi-dense: `VECTOR [[...], [...]]` and upsert `vector: { colbert: [[...]] }`

Do **not** invent name-based heuristics (`*sparse*` → sparse). Kind comes from schema or `AS …`.

## What To Say

Prefer plain language:

- `QQL does not support this yet.`
- `This needs raw Qdrant SDK usage or a QQL extension.`
- `The closest supported QQL form is ...`

## Practical Fallbacks

- Need exact baseline: use `PARAMS (exact = true)`
- Need single point by exact ID: use `QUERY POINTS (42, 'uuid') FROM <collection> WITH PAYLOAD true`
- Need to browse or export points page by page: use `SCROLL FROM <collection> ... LIMIT <n>`
- Need pagination without similarity score: use `QUERY ORDER BY <field> [ASC|DESC] FROM <collection> LIMIT <n>`
- Need to filter returned fields: use `WITH PAYLOAD INCLUDE ('f1', 'f2') WITH VECTOR ('name')`
- Need recall tuning: use `PARAMS (hnsw_ef = ...)`
- Need flat search pagination: use `QUERY ... LIMIT <n> OFFSET <n>`
- Need low-score filtering: use `QUERY ... SCORE THRESHOLD <float>`
- Need cross-collection lookup: use `QUERY ... LOOKUP FROM <collection>`
- Need keyword plus semantic retrieval: use `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION RRF FROM <collection> LIMIT <n>`
- Need parameterized RRF tuning: use `PARAMS (rrf_k = <n>, rrf_weights = [...])`
- Need multi-stage retrieval with per-prefetch filters: use `WITH <name> AS (...) ... PREFETCH (name WHERE <filter> SCORE THRESHOLD <n>) FUSION RRF`
- Need hybrid DBSF fusion: use `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION DBSF FROM <collection> LIMIT <n>`
- Need better ordering / late interaction: use `QUERY RERANK TEXT 'query' MODEL 'colbert-model' FROM <collection> USING colbert PREFETCH (...) LIMIT <n>` (collection needs multivector `colbert`; host must implement `embed_multi`)
- Need multivector nearest without schema: use `USING colbert AS MULTI`
- Need multivector nearest with schema: use `USING colbert` (no `AS` required)
- Need filtering: create an index first (`CREATE INDEX ON COLLECTION <name> FOR <field> TYPE <type>`), then use `WHERE`
- Need grouped top results by field: use `QUERY ... GROUP BY <field> SIZE <n>`
- Need cross-collection group lookup: use `QUERY ... GROUP BY <field> SIZE <n> LOOKUP FROM <collection>`
- Need to patch metadata in place: use `UPDATE <collection> SET PAYLOAD = {...} WHERE ...`
- Need to replace a stored vector: use `UPDATE <collection> SET VECTOR <name> = [...] WHERE id = ...`
- Need a runnable prototype: stay inside `CREATE`, `CREATE INDEX`, `UPSERT`, `QUERY`, `DELETE`
- Need batch upsert: use comma-separated `UPSERT INTO <name> VALUES {...}, {...}`
- Need script round-trip: use `qql execute <file.qql>` and `qql dump <collection> <output.qql>`
- Need local inference without cloud: configure an embedder and set `USING DENSE MODEL` / `USING HYBRID`
- Need score shaping: use `QUERY FORMULA score * 2 DEFAULTS (score = 0.0) FROM <collection> USING dense LIMIT <n>`
- Need random sampling: use `QUERY SAMPLE RANDOM FROM <collection> LIMIT <n>`
- Need geo-distance decay: use `QUERY FORMULA ...` with decay functions
- Need conditional scoring: use `QUERY FORMULA ...` with CASE expressions
- Need mathematical score shaping: use `QUERY FORMULA sqrt(score) * log(citations + 1) FROM ...`
- Need multi-tenant isolation: use `SHARD '<key>'` on QUERY, UPSERT, SCROLL, DELETE
- Need quantization-aware search: use `PARAMS (quantization = {ignore: false, rescore: true, oversampling: 2.0})`
- Need API key authentication: pass `api_key` to SDK Client or set `QDRANT_API_KEY` env var

## Reminder

Do not hide missing features behind made-up syntax. If the current CLI cannot parse and execute it, it is outside this skill.
