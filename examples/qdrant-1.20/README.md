# Qdrant 1.20 examples: BM25 without embeddings, hybrid, code search

> **DRAFT.** The server-side BM25 statements here use the **proposed** QQL
> syntax from [`docs/design/bm25-server-side-1.20.md`](../../docs/design/bm25-server-side-1.20.md).
> (`CREATE INDEX ON COLLECTION c FOR field TYPE text WITH (scoring = 'bm25', tokenizer = …)` and `QUERY BM25 '…' FROM c USING field PARAMS (k = …, b = …)`, as locked by Core Rust). They do not parse on `dev` until Core Rust lands the BM25 grammar. Each
> script checks `is_valid` first and prints `pending QQL grammar` instead of
> failing, so CI and curious users can run them today. Each file keeps its
> QQL in one constant, so switching to Core's final spelling is a one-line change.

| Script | Path | Needs |
|--------|------|-------|
| [`bm25_text_search.py`](bm25_text_search.py) | **Server BM25**: full-text index with `scoring = 'bm25'` plus a `QUERY BM25` query, no embedder at all | Qdrant ≥ 1.20 |
| [`hybrid_dense_bm25.mjs`](hybrid_dense_bm25.mjs) | **Hybrid**: dense leg plus server-BM25 leg, RRF fusion (Node) | Qdrant ≥ 1.20 and an embedder for the dense leg |
| [`code_search.py`](code_search.py) | **Code search**: `tokenizer = 'code'` plus BM25, with a `SUBSTRING` path filter | Qdrant ≥ 1.20 |
| [`edge_client_sparse_bm25.py`](edge_client_sparse_bm25.py) | **Client sparse BM25** (works today): edge, Qdrant < 1.20, existing sparse collections | any Qdrant ≥ 1.19, or qdrant-edge 0.8 |

Which path to use:

- **Remote Qdrant ≥ 1.20, new collection** → server BM25. You don't need a
  sparse vector or a write-path encoder, and `k`/`b` are set per query.
- **qdrant-edge, Qdrant < 1.20, or an existing `qdrant/bm25` sparse vector** →
  client sparse BM25 (`USING sparse` / `AS SPARSE`). Tune it with the
  embedder's `bm25_k1` / `bm25_b` / `bm25_avg_len`. Those options apply only to
  this path.
- A query leg uses one BM25 path, never both, so nothing is scored twice.

Run offline (parse/compile only):

```bash
python3 examples/qdrant-1.20/bm25_text_search.py
node examples/qdrant-1.20/hybrid_dense_bm25.mjs
```

Run live against Qdrant 1.20:

```bash
docker run -p 6333:6333 qdrant/qdrant:v1.20.0
QDRANT_URL=http://localhost:6333 python3 examples/qdrant-1.20/bm25_text_search.py
```
