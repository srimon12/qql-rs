# qql-embed

Shared embedding resolution layer. Contains the host-agnostic [`Embedder`] trait,
a hash-based BM25 [`SparseEmbedder`], and [`resolve_embeddings`] — the recursive
AST rewriter that converts text query/upsert inputs into vectors.

No Qdrant I/O, no HTTP client, no transport code. Used by `qql-runtime`
(`HttpEmbedder`), `qql-edge` (`FastEmbedder`), and `qql-wasm` (JS/fetch adapters).

## Embedder trait

```rust
pub trait Embedder: Send + Sync {
    /// Dense embedding — batch API, grouped by model.
    async fn embed_dense_batch(&self, texts: &[String], model: &str) -> Result<Vec<Vec<f32>>>;
    /// Sparse embedding — per-item BM25 (local, no network).
    async fn embed_sparse(&self, text: &str) -> Result<SparseVector>;
}
```

Dense embedding is **always batched by model** — every statement's text inputs
are collected into `EmbeddingJob` structs (`qql-plan::embedding::extract_jobs`),
grouped by model name, and sent as one batch per model.

## resolve_embeddings — AST rewriter

```rust
use qql_embed::{resolve_embeddings, DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME};

let mut stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'}").unwrap();
resolve_embeddings(&mut stmt, &embedder).await?;
// stmt now has text → dense vector for point[0]
```

Resolution happens in these cases:

| Statement | Input source | Output |
|-----------|-------------|--------|
| `QUERY 'text'` | Bare string or `TEXT '...'` | Query input rewrites to dense vector |
| `QUERY HYBRID TEXT '...'` | Hybrid text | Dense + sparse vector pair |
| `UPSERT ... USING DENSE MODEL 'm'` | Payload `text` field | Dense vector per point |
| `UPSERT ... USING HYBRID` | Payload `text` field | Dense + sparse vectors per point |
| `UPSERT ... EMBED title INTO vec` | Explicit source field | Dense/sparse via `embed` directive |
| Auto-embed (no USING) | Payload `text`/`body`/`content` → default `dense` + `sparse` vectors | Dense + sparse |
| `UPSERT` with explicit `VECTOR` | — | No embedding needed |
| `QUERY NEAREST VECTOR [...]` | — | No embedding needed |
| `QUERY NEAREST POINT 42` | — | No embedding needed |

### Default vector names

- `DENSE_VECTOR_NAME`: `"dense"` (constant)
- `SPARSE_VECTOR_NAME`: `"sparse"` (constant)

These are used when no explicit `USING` name is specified in `UPSERT ... EMBED`.

## SparseEmbedder — local BM25

Hash-based term-frequency tokenizer with IDF-like weighting. No network, no model
downloads, no external dependencies. Used automatically as the sparse embedding
backend for hybrid queries and hybrid upserts.

```rust
use qql_embed::SparseEmbedder;

let embedder = SparseEmbedder::new();
let sv = embedder.embed_sparse("quantum computing").await?;
// sv.indices: [u32; N], sv.values: [f32; N]
```

## Known WASM limitation

`qql-wasm` re-exports `qql-embed` types but the `#[wasm_bindgen]` API surface
does not include `resolve_embeddings` — the WASM runtime (Client) delegates
embedding to JS-side HTTP calls. See `qql-wasm/src/lib.rs` for details.

## Features

- `std` (default): `std::error::Error` impl
- All types are `Send + Sync` on non-wasm targets; `?Send` on wasm32

## Verification

```bash
cargo test -p qql-embed -- --test-threads=4
```

Tests cover:
- Dense query text → vector resolution
- Hybrid query (dense + sparse) resolution
- UPSERT text payload → dense/sparse auto-embedding
- UPSERT USING DENSE MODEL / HYBRID resolution
- EMBBED directive with explicit source field and target vector name
- Sparse BM25 tokenization and IDF weighting
- Unused embedding detection (dense_iter exhaustion check)
