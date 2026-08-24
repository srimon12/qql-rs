# The QQL Story

*Three implementations. Four months. One language for vector search.*

---

## Today (Rust workspace)

The live product is this monorepo (`qql-rs`): `qql-core` → `qql-plan` → `qql-runtime` / edge,
with `pyqql`, `nqql`, `qql-wasm`, and `qql-cli`. Language **1.2** adds hybrid shorthand,
multivector, CLIP, cross-encoder, request `timeout`/`consistency`, and first-class
`SHARD` routing alongside `CREATE SHARD KEY` DDL. Canonical docs: [README](README.md).

---

## Origins: The Python Prototype

**Repo:** `/data/codebases/qql` — [github.com/pavanjava/qql](https://github.com/pavanjava/qql)

QQL began in April 2026 as a simple question: *"What if we could talk to Qdrant like we talk to Postgres?"* Kameshwara Pavan Kumar Mantha built the first version in Python — a SQL-like CLI and library for the Qdrant vector database. Srimon Danguria joined as the second-highest contributor.

The Python `qql-cli` was a proof-of-concept that proved three things:

1. **A query language for vector search is useful.** Writing raw `QdrantClient.search()` with deeply nested filter dicts is painful. `SEARCH 'text' FROM docs WHERE category = 'tech' LIMIT 10` is obvious.
2. **Local embedding via Fastembed works.** Text goes in, vectors come out — no external API call needed. The embedding pipeline could run entirely on the developer's machine.
3. **CLI + interactive REPL is the right UX.** `qql connect` gave an instant, syntax-highlighted shell for vector search.

The Python version shipped 14 releases in 10 weeks, from v0.1.0 to v2.6.0, with 549+ tests. It supported INSERT, SEARCH, SELECT, RECOMMEND, SCROLL, UPDATE, DELETE, CREATE COLLECTION, hybrid dense+sparse search, GROUP BY, cross-encoder reranking, quantization, and script file execution.

But Python had limits. It was tied to the Python runtime. Startup was slow. No gRPC. No server story. And the language syntax was loose — separate keywords for SEARCH, SELECT, and RECOMMEND rather than a unified query model.

---

## The Pivot: Go as a Platform

**Repo:** `/data/codebases/qql-go` — [github.com/srimon12/qql-go](https://github.com/srimon12/qql-go)

Eight days after the Python project began, Srimon started the Go port. Single author. 201 commits. 12 releases. This is where QQL stopped being a tool and became a **platform**.

### What changed architecturally

| Python | Go |
|--------|-----|
| Map-based keyword lookup | Generated O(1) switch table (Bolt-optimized, ~192 ns/op) |
| Recursive descent parser | Pratt parser, zero-alloc byte-level comparison |
| Direct SDK calls | Pipeline DAG pattern (EmbedNode → FusionNode → RerankNode) |
| REST-only | gRPC native via Connect RPC |
| Python only | Go + Python SDK + TypeScript SDK |
| CLI tool | CLI + Gateway + MCP Server + SDKs |

### The Gateway — the biggest innovation

`qql-go serve` exposed 5 RPCs (Exec, ExecBatch, Explain, Health, Convert) over gRPC, gRPC-Web, and HTTP/1.1 JSON — all from the same protobuf service definition. It wasn't just a query proxy. It was a **policy enforcement point**:

- **JWT authentication** against any IdP (Auth0, Okta, Keycloak, Firebase, Azure AD, Cognito)
- **AST injection** — tenant filters rewritten into the query before execution, not filtered after results come back. Query-level security, not result-level.
- **Policy engine** — operation allow/deny lists, collection glob patterns, LIMIT caps, complexity guards (filter depth, OR fan-out, prefetch depth)
- **Rate limiting** with per-user and anonymous tiers
- **Audit logging** — structured JSON entries for every query
- **Policy hot-reload** — zero downtime, fsnotify-based

This is a genuinely hard security problem solved at the right layer. Most vector database gateways filter results after retrieval. QQL rewrites the query AST before it hits Qdrant — the tenant literally cannot see data they shouldn't.

### The MCP Server

`qql-go mcp` exposed QQL as 5 MCP tools + 3 document resources for LLM agents. Built before MCP was widely adopted, it let AI agents execute vector queries safely with policy enforcement.

### Language evolution

The Go version unified the language under a single `QUERY` keyword:

```sql
-- Python v1 (separate keywords)
SEARCH 'text' FROM docs LIMIT 10;
RECOMMEND POSITIVE (1, 2) FROM docs LIMIT 10;

-- Go v0.3+ (unified QUERY)
QUERY 'text' FROM docs LIMIT 10;
QUERY RECOMMEND POSITIVE (1, 2) FROM docs LIMIT 10;
QUERY HYBRID ... FUSION RRF ...
QUERY FORMULA $score * 2 + 0.3 * popularity ...
```

It added CTEs (`WITH ... AS (...)`) for manual prefetch DAGs, a full formula expression engine (arithmetic, math functions, geo-distance, decay functions, CASE WHEN), parameterized RRF, ORDER BY, SAMPLE RANDOM, and relevance feedback. This was the version that proved QQL could express *any* vector database operation, not just basic search.

---

## The Rewrite: Rust as a Universal SDK

**Repo:** `/data/codebases/qql-rs` — [github.com/srimon12/qql-rs](https://github.com/srimon12/qql-rs)

Srimon rewrote everything in Rust. 119 commits in 22 days. 11 workspace crates. Not a port — a complete re-architecture informed by every lesson from Python and Go.

### The Three-Layer Architecture

```
qql-core   → parse, AST, inject_filter, explain, tokenize   (no_std, ZERO deps)
qql-plan   → AST → typed Route { method, path, body }        (no I/O)
qql-runtime → execute PlannedOperation via REST/gRPC/Edge     (async)
                    ↓
            Single QdrantOps trait, 3 impls:
            RestQdrant | GrpcQdrant | EdgeQdrant
```

The critical insight: **separate parsing from planning from execution.** Python and Go had these tangled. Rust enforces a clean boundary:

- `qql-core` is `no_std`, zero external dependencies. You can parse, validate, explain, and inject filters into QQL *on a microcontroller*. This is what makes WASM and edge work.
- `qql-plan` is pure transformation — no I/O, no network, just AST → typed PlannedOperation.
- `qql-runtime` has a single `QdrantOps` trait. No code duplication, no switch-on-backend.

### What Rust unlocked that was impossible before

**1. `qql-edge` — In-process vector search, no Qdrant server**

Uses qdrant-edge for local HNSW storage and fastembed-rs for ONNX inference. Entire QQL pipeline runs in a single process. Usable in CLI, desktop apps, mobile, edge devices. No network. No server. No Docker.

```bash
qql config edge --data-dir ./qql-data --model bge-small-en-v1.5
qql --edge exec "QUERY 'search' FROM docs LIMIT 10"
```

**2. Four language bindings from ONE codebase**

| Binding | Crate | Distribution |
|---------|-------|-------------|
| Python | `pyqql`, `pyqql-edge` | PyPI (`pip install pyqql`) |
| Node.js | `nqql`, `nqql-edge` | npm (`npm install @veristamp/nqql`) |
| WASM | `qql-wasm` | npm (browser, 424 KB compressed) |
| Rust | `qql-cli`, `qql` | crates.io (`cargo install qql-cli`) |

Fix a bug in one place — every binding gets the fix. Same parser, same semantics, same error codes.

**3. WASM playground** — [github.com/srimon12/qql-wasm-demo](https://github.com/srimon12/qql-wasm-demo)

Interactive browser-based QQL editor with CodeMirror 6, 163-keyword syntax highlighting, live linting via WASM `analyze()`, autocompletion, in-browser MiniLM embeddings via Transformers.js, and a policy sandbox for multi-tenant queries. A complete QQL IDE in a browser tab.

**4. VS Code extension** — `editors/vscode/`

Syntax highlighting (TextMate grammar with 170 keywords), live diagnostics (same WASM parser), and 19 snippet templates. 850 KB VSIX, zero external dependencies.

**5. Language specification** — `language/v1/grammar.pest`

A canonical PEG spec — 688 lines, 19 statement types, 14 query expressions. The reference parser in `qql-core` is hand-written (lexer + `AstLowerer`); pest is **not** compiled into the runtime, it exists only as a test-only harness in `qql-conformance` that executes `grammar.pest` against the fixture corpus. The spec is the authority — implementations derive from it, not the other way around. Conformance fixtures: 39 valid `.qql` files (265 statements), 56 invalid cases, and 39 canonical AST snapshots, over 170 grammar keywords.

### Published artifacts (v0.1.2)

| Registry | Packages |
|----------|----------|
| **crates.io** | `qql-core`, `qql-plan`, `qql-embed`, `qql`, `qql-edge`, `qql-cli` |
| **PyPI** | `pyqql`, `pyqql-edge` |
| **npm** | `nqql`, `nqql-edge`, `qql-wasm` + platform packages |
| **GitHub Releases** | CLI archives (Linux, macOS Intel, macOS ARM, Windows) |

---

## The Evolution Arc

| Python (Apr 6 – Jun 13) | Go (Apr 14 – Jun 30) | Rust (Jul 4 – Jul 25) |
|---|---|---|
| CLI + library | CLI + Gateway + SDKs + MCP | Universal SDK + Edge + WASM + Extension |
| Fastembed (Python) | Remote API only | Fastembed-rs (local) + Remote + Browser |
| No auth/security | JWT + Policy Engine + AST Injection | Same injection logic, reusable everywhere |
| pip install | Single Go binary | 5 MB CLI + PyPI + npm + WASM + VS Code |
| 549 tests | Feature parity | 357 tests + conformance suite |

---

## What QQL Actually Is

QQL is a **domain-specific query language for vector search on Qdrant**. It is to Qdrant what SQL is to Postgres.

```sql
-- Semantic search
QUERY 'what is vector search' FROM docs USING dense LIMIT 10;

-- Hybrid search with Reciprocal Rank Fusion
QUERY HYBRID TEXT 'vector database' DENSE dense SPARSE bm25 FUSION RRF
FROM docs LIMIT 10;

-- Multi-stage retrieval with CTEs
WITH
  dense AS (QUERY TEXT 'ml' USING dense LIMIT 100),
  sparse AS (QUERY TEXT 'ml' USING bm25 LIMIT 100)
QUERY FUSION RRF FROM docs PREFETCH (dense, sparse) LIMIT 10;

-- Formula-based scoring
QUERY FORMULA $score * 2 + 0.3 * popularity
  DEFAULTS (score = 0.0) FROM docs LIMIT 10;

-- Multi-tenant isolation with shard routing
QUERY 'supply chain' FROM sec10k
  WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 10;
```

---

## Links

| What | Where |
|------|-------|
| **Rust implementation** (reference) | `/data/codebases/qql-rs` — [github.com/srimon12/qql-rs](https://github.com/srimon12/qql-rs) |
| **Go implementation** (gateway) | `/data/codebases/qql-go` — [github.com/srimon12/qql-go](https://github.com/srimon12/qql-go) |
| **Python implementation** (original) | `/data/codebases/qql` — [github.com/pavanjava/qql](https://github.com/pavanjava/qql) |
| **Language specification** | `qql-rs/language/v1/grammar.pest` |
| **VS Code extension** | `qql-rs/editors/vscode/` |
| **Browser playground** | [github.com/srimon12/qql-wasm-demo](https://github.com/srimon12/qql-wasm-demo) |
| **Agent skill** | `qql-rs/skills/qql-skill/` |
| **Conformance fixtures** | `qql-rs/language/v1/` |

---

*Built by Kameshwara Pavan Kumar Mantha (Python) and Srimon Danguria (Python, Go, Rust). April – July 2026.*
