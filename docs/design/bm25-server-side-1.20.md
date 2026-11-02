# BM25 on Qdrant 1.20: server-side text index vs client-side sparse

> **Status: grammar landed (Core, PR #240).** `QUERY BM25 '…' FROM c USING field
> PARAMS (k = …, b = …)` and `CREATE INDEX … TYPE text WITH (scoring = 'bm25',
> tokenizer = 'code', …)` now parse, plan, and route (REST + gRPC). Core used
> `QQL-VALIDATION-BM25-USING` / `QQL-VALIDATION-BM25-PARAMS` (rather than a single
> `QQL-VALIDATION-BM25`), `QQL-PLAN-BM25`, `QQL-EDGE-UNSUPPORTED-TEXT-BM25`,
> `QQL-EDGE-UNSUPPORTED-CODE-TOKENIZER`, and `QQL-BACKEND-VERSION`. The rest of
> this note is the original SDK-side contract (routing rules, binding options,
> examples, and `qql convert` mappings, which still fail closed).

## Background

QQL already ships **client-side BM25**. `qql-embed` vendors Qdrant's BM25
pipeline (`Bm25Pipeline`, model `qdrant/bm25`, wire-compatible token IDs), so
QQL computes sparse document vectors on the write path and sparse query vectors
on the read path, then searches a sparse named vector (`USING sparse`,
`USING name AS SPARSE`, `QUERY HYBRID … SPARSE bm25`).

Qdrant **1.20** adds **server-side BM25 over full-text payload indexes**:

```http
PUT /collections/docs/index
{"field_name": "title", "field_schema": {"type": "text", "scoring": {"type": "bm25"}}}

POST /collections/docs/points/query
{"query": {"text": {"query": "time travel", "scoring": {"bm25": {"k": 1.2, "b": 0.75}}}},
 "using": "title", "limit": 10}
```

Server-side BM25 needs no sparse vector, no write-path encoder, and no
re-upload: the index records document lengths, and `k` and `b` are set per
query. IDF can be scoped with the existing `params.idf` corpus filter.

## Routing rules

| # | Situation | Path | Wire |
|---|-----------|------|------|
| 1 | Remote Qdrant **≥ 1.20**, `QUERY BM25 '…' USING <field>`, where the field's text index has `scoring = 'bm25'` | **Server BM25** | `{"text": …}` query, `using: <field>` |
| 2 | Remote Qdrant **< 1.20** | Client sparse BM25 (unchanged) | `{"nearest": {indices, values}}` on a sparse vector |
| 3 | **qdrant-edge** (`pyqql-edge`, `nqql-edge`, `qql-edge`) | Client sparse BM25 (unchanged) | local sparse search |
| 4 | Any server, `USING sparse` / `AS SPARSE` / `MODEL 'qdrant/bm25'` on an existing **sparse vector** | Client sparse BM25 (unchanged) | sparse nearest |

Invariants:

- **No double scoring.** A query leg runs exactly one BM25 path. A server
  `text` leg never also encodes a client sparse vector, and a client sparse
  leg never also emits a `text` query. In a hybrid, each `PREFETCH` leg is
  routed on its own, so a dense + server-BM25 hybrid sends one `nearest` leg and
  one `text` leg.
- **Explicit beats inferred.** `QUERY TEXT` keeps meaning "embed this text into
  a vector". Server BM25 has its own expression keyword, `QUERY BM25`, whose
  `USING` names a *payload field*, not a vector. Neither form reinterprets the
  other, so upgrading the server never changes how an existing query runs.
- **Existing collections keep working.** No query that runs today changes
  path when the server is upgraded to 1.20. Server BM25 is opt-in per query.

## Errors (fail closed, before any request is sent)

| Case | Proposed code | Message sketch |
|------|---------------|----------------|
| Server BM25 on qdrant-edge 0.8 | `QQL-EDGE-UNSUPPORTED-TEXT-BM25` | "`QUERY BM25` needs remote Qdrant ≥ 1.20; on edge use a sparse vector with `QUERY TEXT … USING sparse`" |
| Server BM25 against Qdrant < 1.20 (version known from `/` or telemetry) | `QQL-BACKEND-VERSION` | "server-side BM25 needs Qdrant ≥ 1.20 (server reports 1.19.x)" |
| `QUERY BM25` field has no text index, or the index has no `scoring` | `QQL-VALIDATION-TEXT-INDEX` | "field 'title' has no BM25-scored text index; `CREATE INDEX ON COLLECTION c FOR title TYPE text WITH (scoring = 'bm25')`" |
| `k` < 0, or `b` outside [0, 1] | `QQL-VALIDATION-BM25` | mirrors Qdrant `Bm25Params` bounds |
| Client-side tuning (`bm25_k1`, …) combined with a `QUERY BM25` leg | none (no error) | client options only shape the sparse encoder; they never change a server leg |

The codes are proposals. Core Rust owns the final registry.

## SDK options: which knob applies where

| Knob | Python / Node / WASM | Applies to |
|------|----------------------|------------|
| `bm25_k1` / `bm25K1`, `bm25_b` / `bm25B`, `bm25_avg_len` / `bm25AvgLen`, `setBm25Params(k1, b, avgLen)` | `HttpEmbedder(...)`, Node `embedder: {...}`, edge `httpExecutor(..., bm25Options)`, WASM `Client.setBm25Params` | **Client sparse path only** (rules 2 to 4): the local document encoder on the write path. These options never touch a server `text` query. |
| `bm25_language`, `bm25_tokenizer`, `bm25_stopwords`, `bm25_stemmer`, … | same | Client sparse path only |
| `CREATE INDEX … TYPE text WITH (scoring = 'bm25', tokenizer = …, stemmer = …, stopwords = …)` | QQL text | **Server path** (rule 1): tokenization and length stats live in the Qdrant index |
| `QUERY BM25 … PARAMS (k = …, b = …)` | QQL text | **Server path** only. Defaults are `1.2` / `0.75` (Qdrant) |
| `PARAMS (idf = 'global' \| WHERE …)` | QQL text | Both paths. For server BM25 the corpus filter also scopes the average document length |

No binding signature changes are needed: server BM25 is pure QQL text, so it
works through every binding's `execute()` once the grammar lands.

## QQL surface (locked by Core Rust)

```sql
-- Index: BM25 scoring on a full-text index (existing WITH (…) option list)
CREATE INDEX ON COLLECTION books FOR title TYPE text
  WITH (scoring = 'bm25', tokenizer = 'word', lowercase = true);

-- Code search tokenizer (1.20)
CREATE INDEX ON COLLECTION repo FOR body TYPE text
  WITH (scoring = 'bm25', tokenizer = 'code', lowercase = true);

-- Server BM25 query, no embeddings
QUERY BM25 'time travel' FROM books USING title LIMIT 10;
QUERY BM25 'time travel' FROM books USING title PARAMS (k = 1.2, b = 0.75) LIMIT 10;

-- Hybrid: dense leg (QUERY TEXT embeds) + server BM25 leg (payload field), fused
WITH
  dense AS (QUERY TEXT 'time travel' USING dense LIMIT 50),
  lexical AS (QUERY BM25 'time travel' USING title PARAMS (k = 1.2, b = 0.75) LIMIT 50)
QUERY FUSION RRF FROM books PREFETCH (dense, lexical) LIMIT 10;
```

`k` and `b` share the `PARAMS (…)` block with `idf`, so a scoped corpus reads
`PARAMS (k = 1.2, b = 0.75, idf = WHERE tenant_id = 'acme')`.

Still open, for Core Rust:

1. **`QUERY HYBRID` sugar.** Should there be a shorthand with server BM25 as the
   lexical leg, or only the explicit `PREFETCH` form above?
2. **Planner placement of `k`/`b`.** `PARAMS (k, b)` lowers into the query's
   `text.scoring.bm25`, not into `params` (Qdrant `SearchParams`). On a
   non-BM25 query, `k`/`b` should fail closed.

## `qql convert` mappings (activate with the grammar)

`qql convert` currently **fails closed** on every 1.20 shape with an
`InvalidField` whose detail starts with `Qdrant 1.20 …`. See
`crates/qql-convert/src/decode/pending.rs` and
`crates/qql-convert/tests/qdrant_1_20_pending.rs`. Once the AST exists, each
row below replaces a `pending(...)` arm, and its test changes to a round trip.

| Qdrant 1.20 REST | QQL (proposed) |
|------------------|----------------|
| `{"query": {"text": "q"}, "using": "title"}` | `QUERY BM25 'q' FROM c USING title` |
| `{"query": {"text": {"query": "q", "scoring": {"bm25": {"k": 1.5, "b": 0.5}}}}, "using": "title"}` | `QUERY BM25 'q' FROM c USING title PARAMS (k = 1.5, b = 0.5)` |
| `prefetch[i].query = {"text": …}` | the same leg inside `PREFETCH` / `WITH … AS (…)` |
| `field_schema.scoring = {"type": "bm25"}` or `true` | `WITH (scoring = 'bm25')` |
| `field_schema.scoring = false` | option omitted |
| `field_schema.tokenizer = "code"` | `WITH (tokenizer = 'code')` |
| `field_schema.nested = "offers"` | `WITH (nested = 'offers')` |
| `match.substring = "src/"` | `WHERE path SUBSTRING 'src/'` (Core's proposed operator) |
| `datatype: "turbo8"` / `"turbo16"` | `WITH VECTOR (datatype = 'turbo8')` |
| `quantization_config.turbo.bits = "bits8"` | `WITH QUANTIZATION (type = 'turbo', bits = 8)` |
| `id_tracker.memory` | `WITH PARAMS (id_tracker_memory = 'cold')` (TBD by Core) |

A `{"text": …}` object **with** `model` is still an inference `Document` and
keeps converting as it does today. The 1.20 guard only claims `text` objects
without `model`.

## Rollout

1. **Foundation sync** (Core Rust): vendor the v1.20.0 protos and `openapi.json`, then regenerate.
2. **BM25 grammar + planner** (Core Rust), plus in the same PR: SDK examples
   under `examples/qdrant-1.20/` move out of draft, `qql convert` mappings
   switch on, and docs and VS Code updates land (Frontend).
3. CI runs a Qdrant 1.19 and 1.20 matrix. Server-BM25 tests skip below 1.20,
   and a 1.19 run asserts the `QQL-BACKEND-VERSION` error.
