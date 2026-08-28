# qql-embed

Shared embedding resolution: host-agnostic [`Embedder`] trait, local BM25
[`SparseEmbedder`], [`resolve_embeddings`], and schema
[`resolve_query_vector_kinds`].

## Proposition

**One embed owner** for every backend. No Qdrant I/O, no HTTP client. Runtime
(`HttpEmbedder`), edge (`FastEmbedder`), and WASM adapters implement the trait.
Schema fills `USING` kinds **before** embed; unknown kinds fail closed
(`QQL-VECTOR-KIND`) — never silent dense defaults for named vectors.

## Embedder trait

```rust
pub trait Embedder: Send + Sync {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>>;
    /// Sparse embedding for query text (default: local wire-compatible BM25, unit weights).
    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector>;
    /// Sparse embedding for document text (default: local wire-compatible BM25, tf saturation).
    async fn embed_sparse_document(&self, text: &str, model: &str) -> Result<SparseVector>;
    /// Batch document-side sparse embedding. Default loops `embed_sparse_document`.
    async fn embed_sparse_document_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>>;
    /// Dense embedding — batch API, grouped by model.
    async fn embed_dense_batch(&self, texts: &[String], model: &str) -> Result<Vec<Vec<f32>>>;
    /// Multivector (ColBERT-style). Default rejects with QQL-EMBEDDING-MULTI.
    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>>;
    async fn embed_multi_batch(&self, texts: &[String], model: &str) -> Result<Vec<Vec<Vec<f32>>>>;
    /// Image / CLIP vision embedding. Default rejects with QQL-EMBEDDING-IMAGE.
    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>>;
    /// Cross-encoder pair scoring: (query, documents[i]) → scores. Default rejects with QQL-RERANK-CROSS.
    async fn rerank_pairs(&self, query: &str, documents: &[String], model: &str) -> Result<Vec<f32>>;
    /// Single-pass joint embeddings (dense + sparse + multi in one pass for BGE-M3).
    async fn embed_joint(&self, text: &str, model: &str) -> Result<JointEmbeddingOutput>;
    async fn embed_joint_batch(&self, texts: &[String], model: &str) -> Result<Vec<JointEmbeddingOutput>>;
}
```

Dense embedding is **batched by model** when the target is single-vector dense.
Sparse is role-split: queries embed with unit term weights
(`embed_sparse_query`), documents with BM25 term-frequency saturation
(`embed_sparse_document`) — both matching Qdrant's `qdrant/bm25` defaults.
Multivector defaults reject until the host opts in (`embed_multi`), as does
image embedding (`embed_image`).

### FastEmbed-style host mapping

| Host capability | QQL method | Shape |
|---|---|---|
| Sentence / CLIP **text** dense (`TextEmbedding`) | `embed_dense` | `[f32]` |
| CLIP **vision** / image dense (`ImageEmbedding`) | `embed_image` | `[f32]` |
| Sparse query (BM25 / SPLADE) | `embed_sparse_query` | indices + values |
| Sparse document (BM25 / SPLADE) | `embed_sparse_document` / `_batch` | indices + values |
| ColBERT / BGE-M3 **ColBERT** bags (`Bgem3Embedding.colbert`) | `embed_multi` | `[[f32],…]` |
| Cross-encoder pair scores (`TextRerank`) | `rerank_pairs` | per-document `[f32]` |

CLIP is dual-encoder **dense**, never multivector. Multivector is late-interaction bags only.

Language:

- `QUERY IMAGE 'path-or-url' [MODEL '…']` → `embed_image` → `Dense`
- `UPSERT … USING IMAGE MODEL '…' ON FIELD image INTO image`

## Schema topology before embed

Parse leaves `USING name` as `kind: null`. Execution prep must fill kinds:

```rust
use qql_embed::{resolve_query_vector_kinds, resolve_embeddings, TopologyNames};

// From collection schema (runtime / WASM):
let topology = TopologyNames {
    dense: vec!["dense".into(), "colbert".into()],
    sparse: vec!["sparse".into()],
    multivector: vec!["colbert".into()], // dense names with multivector_config
};
resolve_query_vector_kinds("docs", &mut query, &topology)?;
resolve_embeddings(&mut stmt, &embedder).await?;
```

| After topology | TEXT embed result |
|---|---|
| kind Dense, multi false | `Dense([f32…])` via `embed_dense_batch` |
| kind Sparse | `Sparse { indices, values }` via `embed_sparse` |
| kind Dense, multi true | `MultiDense([[f32…],…])` via `embed_multi` |
| kind still null | **`QQL-VECTOR-KIND`** — never silent dense default |

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
| `QUERY 'text' ... USING name AS DENSE` | Bare string or `TEXT '...'` | Dense vector |
| `QUERY 'text' ... USING name AS SPARSE` | Bare string or `TEXT '...'` | Sparse vector |
| `QUERY 'text' ... USING name AS MULTI` | Bare string or `TEXT '...'` | Multivector → `MultiDense` |
| `QUERY 'text' ... USING name` (no `AS`) | Bare string or `TEXT '...'` | **Errors** unless kinds were filled by `resolve_query_vector_kinds` first (schema may set multivector) |
| `QUERY RERANK TEXT … MODEL 'm' USING colbert` | Rerank text | Dense or MultiDense using model `m` |
| `QUERY HYBRID TEXT '...'` | Hybrid text | Dense + sparse pair expanded to Fusion |
| `UPSERT ... USING DENSE MODEL 'm'` | Payload text field | Dense vector per point |
| `UPSERT ... USING HYBRID` | Payload text field | Dense + sparse vectors per point |
| `UPSERT ... EMBED title INTO vec` | Explicit source field | Dense/sparse via `embed` directive |
| Auto-embed (no USING) | Payload `text`/`body`/`content` | Default dense only |
| Explicit `VECTOR` / `POINT` | — | No embedding |

### Vector roles and default names

Query targets carry an optional role (`DENSE` or `SPARSE`) plus a `multi` flag.
Arbitrary names such as `semantic_v2` and `lexical_v2` are supported; embedding
behavior never depends on a target literally being named `dense` or `sparse`.

- `DENSE_VECTOR_NAME`: `"dense"` (constant)
- `SPARSE_VECTOR_NAME`: `"sparse"` (constant)

These constants are used only when materializing a new default topology.

## SparseEmbedder — local wire-compatible BM25

Client-side BM25 that is **wire-compatible with Qdrant's `qdrant/bm25` model**:
murmur3-32 token IDs (same hash the server uses), word tokenizer (split on
non-alphanumeric), Unicode lowercasing, English stopword removal, and English
snowball stemming — the server's documented defaults. Queries embed with unit
term weights; documents with BM25 tf saturation (k1=1.2, b=0.75, avg_len=256).
IDF is applied server-side via the sparse vector `modifier: idf`. No network,
no model downloads. A synchronous helper backing the default
`Embedder::embed_sparse_query` / `embed_sparse_document` implementations.

```rust
use qql_embed::SparseEmbedder;

let q = SparseEmbedder::embed_query("quantum computing");   // unit weights
let d = SparseEmbedder::embed_document("quantum computing"); // tf saturation
// q/d.indices: [u32; N], q/d.values: [f32; N]
```

Vectors produced here can be mixed with server-side `qdrant/bm25` inference on
the same collection (a golden test pins the exact server output from the
Qdrant docs).

## Known WASM limitation

WASM `Client` prepares statements like the native executor: fetch collection
topology, resolve kinds, then embed (when an embedder is configured). Hosts that
need ColBERT must implement `embed_multi` on their embedder adapter.

## Features

- `std` (default): `std::error::Error` impl
- All types are `Send + Sync` on non-wasm targets; `?Send` on wasm32

## Verification

```bash
cargo test -p qql-embed -- --test-threads=4
```

Tests cover:
- Dense / sparse / multi query resolution
- Fail-closed `USING name` without kind
- Schema multivector → MultiDense
- RERANK + AS MULTI
- Hybrid, UPSERT, EMBED directives
- Sparse BM25 tokenization
