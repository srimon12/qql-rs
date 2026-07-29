# Red teaming branch: bugs, dead code, architecture

**Session ID:** ses_0523ac92effeuHKDoLSlyWNxYP
**Created:** 7/29/2026, 5:36:50 PM
**Updated:** 7/29/2026, 5:44:29 PM

---

## User

so we have been doing a lots of changes in this branch!!
your job is to do the red teaming ..

find bugs, half cooked things, possible architectural cleaniness, possible code removal for clean architecture, dead code, documentation butchering or half cooked docs everywhere..

for this you will need two thing..
1. always refer to the openapi.json and proto files to be in perfectly sync with the contract..
2. for fastembed you will need to look at their codes and @fastembed-rs.md 


so lets get into the code explore properly.. use your subagents properly and find out things!

always remember its not about the branch code.. if something is fishy beyond the changes from the core or anywhere report it..


---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 8.4s)

_Thinking:_

Let me start by understanding the project structure and then systematically explore the codebase to find bugs, half-cooked things, architectural issues, dead code, documentation problems, etc.

Let me first explore the project structure, then look at the openapi.json and proto files, and then dive deep into the code.

**Tool: todowrite**

**Input:**
```json
{
  "todos": [
    {
      "content": "Explore project structure and understand the codebase layout",
      "status": "in_progress",
      "priority": "high"
    },
    {
      "content": "Load openapi.json and proto files for contract verification",
      "status": "pending",
      "priority": "high"
    },
    {
      "content": "Red team review: Find bugs, half-cooked code, dead code, architectural issues",
      "status": "pending",
      "priority": "high"
    },
    {
      "content": "Check fastembed integration against fastembed-rs.md contract",
      "status": "pending",
      "priority": "high"
    },
    {
      "content": "Documentation review: half-cooked docs, missing docs",
      "status": "pending",
      "priority": "medium"
    },
    {
      "content": "Compile findings and report",
      "status": "pending",
      "priority": "high"
    }
  ]
}
```

**Output:**
```
[
  {
    "content": "Explore project structure and understand the codebase layout",
    "status": "in_progress",
    "priority": "high"
  },
  {
    "content": "Load openapi.json and proto files for contract verification",
    "status": "pending",
    "priority": "high"
  },
  {
    "content": "Red team review: Find bugs, half-cooked code, dead code, architectural issues",
    "status": "pending",
    "priority": "high"
  },
  {
    "content": "Check fastembed integration against fastembed-rs.md contract",
    "status": "pending",
    "priority": "high"
  },
  {
    "content": "Documentation review: half-cooked docs, missing docs",
    "status": "pending",
    "priority": "medium"
  },
  {
    "content": "Compile findings and report",
    "status": "pending",
    "priority": "high"
  }
]
```

---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 3.8s)

_Thinking:_

Let me start by exploring the project structure broadly, then dive into specific areas.

Let me start by mapping the project structure and loading the contract files in parallel.

**Tool: read**

**Input:**
```json
{
  "filePath": "/data/codebases/qql-rs"
}
```

**Output:**
```
<path>/data/codebases/qql-rs</path>
<type>directory</type>
<entries>
.fastembed_cache/
.git/
.github/
.gitignore
AGENT.md
bench/
Cargo.lock
Cargo.toml
CHANGELOG.md
CONTRIBUTING.md
crates/
demo/
dist/
docker-compose.yml
docs/
documentation/
editors/
examples/
experiment/
fastembed-rs.md
gaps.md
language/
LICENSE
qdrant_storage/
qdrant-lib-edge/
README.md
RELEASING.md
scripts/
skills/
target/
VERSION

(31 entries)
</entries>
```

**Tool: glob**

**Input:**
```json
{
  "pattern": "**/openapi.json"
}
```

**Output:**
```
/data/codebases/qql-rs/crates/qql-runtime/openapi.json
```

**Tool: glob**

**Input:**
```json
{
  "pattern": "**/*.proto"
}
```

**Output:**
```
/data/codebases/qql-rs/crates/qql-runtime/proto/points_service.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/collections.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/qdrant.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/points.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/collections_service.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/qdrant_common.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/snapshots_service.proto
/data/codebases/qql-rs/crates/qql-runtime/proto/json_with_int.proto
```

**Tool: bash**

**Input:**
```json
{
  "command": "git log --oneline -20"
}
```

**Output:**
```
8d2909b feat: Enhance query handling with timeout and consistency parameters
2a1b1a3 feat: introduce USING HYBRID syntax for hybrid queries
0ccc974 feat: add max_selectivity parameter for ACORN in search params, better error handling and update related documentation
78fc34f feat: add cross-encoder pair scoring with CROSS RERANK syntax
98a9b32 feat: add image embedding support with CLIP vision model
f17827b feat: add support for multivector embeddings in QQL
913dc82 feat: Enhance QQL with multivector support and schema-first embedding resolution
70aca62 feat: add support for multivector embeddings (ColBERT-style)
acf0034 feat: implement schema-first vector kind resolution for queries
0eccf7b feat(cleanup): removed wasm-demo to a seperate github project
1ac19f7 fix: update release workflow for npm Trusted Publishing and improve documentation
db858eb fix: update npm publish commands to specify registry and clean up npmrc files
7404d79 fix: update npm package configuration for public access and improve publish error handling
95c2d12 fix: update package.json for qql-wasm with repository info and public access
ffad03c fix: extract npm package to temporary directory before publishing
03fee70 Merge pull request #15 from srimon12/release/0.1.3
3cff10b docs(changelog): add 0.1.3 release notes
27282f4 chore: update Cargo.lock for version 0.1.3 bump
167816c fix: restore correct platform names in npm optionalDependencies
c252bcd chore: bump version to 0.1.3 and add centralized VERSION file

```

---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 3.9s)

_Thinking:_

Let me explore the crates structure and the key files more deeply.

**Tool: read**

**Input:**
```json
{
  "filePath": "/data/codebases/qql-rs/crates"
}
```

**Output:**
```
<path>/data/codebases/qql-rs/crates</path>
<type>directory</type>
<entries>
nqql-edge/
nqql/
pyqql-edge/
pyqql/
qql-cli/
qql-conformance/
qql-core/
qql-edge/
qql-embed/
qql-grammar-gen/
qql-plan/
qql-runtime/
qql-wasm/

(13 entries)
</entries>
```

**Tool: read**

**Input:**
```json
{
  "filePath": "/data/codebases/qql-rs/Cargo.toml"
}
```

**Output:**
```
<path>/data/codebases/qql-rs/Cargo.toml</path>
<type>file</type>
<content>
1: [workspace]
2: resolver = "2"
3: members = [
4:     "crates/qql-core",
5:     "crates/qql-conformance",
6:     "crates/qql-grammar-gen",
7:     "crates/qql-plan",
8:     "crates/qql-embed",
9:     "crates/qql-runtime",
10:     "crates/qql-cli",
11:     "crates/qql-edge",
12:     "crates/qql-wasm",
13:     "crates/pyqql",
14:     "crates/pyqql-edge",
15:     "crates/nqql",
16:     "crates/nqql-edge",
17: ]
18: 
19: [workspace.package]
20: version = "0.1.3"
21: edition = "2021"
22: rust-version = "1.90"
23: authors = ["Srimon Danguria <srimon12mckv@gmail.com>"]
24: license = "MIT"
25: repository = "https://github.com/srimon12/qql-rs"
26: homepage = "https://github.com/srimon12/qql-rs"
27: 
28: [profile.release]
29: lto = "thin"
30: codegen-units = 1
31: strip = true

(End of file - total 31 lines)
</content>
```

**Tool: read**

**Input:**
```json
{
  "filePath": "/data/codebases/qql-rs/AGENT.md"
}
```

**Output:**
```
<path>/data/codebases/qql-rs/AGENT.md</path>
<type>file</type>
<content>
1: # QQL Agent & Developer Reference Guide
2: 
3: Welcome to the QQL Rust codebase. This guide details the architecture, design philosophy, contract testing standards, and key implementation guidelines for developers and AI coding agents.
4: 
5: ---
6: 
7: ## 1. Workspace Architecture
8: 
9: The workspace is organized into a modular, multi-crate Rust workspace under the `crates/` directory:
10: 
11: ```
12: qql/ (workspace root)
13: ├── crates/
14: │   ├── qql-core/         # Lexer, parser, typed AST, explain, filter injection
15: │   ├── qql-plan/         # Fallible planner: AST → PlannedOperation; REST projection
16: │   ├── qql-embed/        # Shared Embedder trait, sparse BM25, resolve_embeddings (batch dense)
17: │   ├── qql-runtime/      # Executor (package name `qql`), REST & gRPC adapters, HttpEmbedder
18: │   ├── qql-edge/         # Local in-process executor: fastembed-rs + qdrant-edge
19: │   ├── qql-cli/          # CLI binary and interactive REPL
20: │   ├── pyqql/            # Python bindings (PyO3)
21: │   ├── nqql/             # Node.js bindings (N-API)
22: │   └── qql-wasm/         # WebAssembly bindings (wasm-bindgen)
23: ```
24: 
25: ### Execution Pipeline
26: 
27: ```
28: source / host AST
29:     │
30:     ▼
31: qql-core: parse + semantic AST validation
32:     │
33:     ▼
34: qql-runtime: prepare_statement
35:   - named-vector validation + kind/multi from collection schema
36:   - embedding resolution (qql-embed): Dense | Sparse | MultiDense
37:   - upsert collection prep
38:     │
39:     ▼
40: qql-plan: plan() → Result<PlannedOperation, PlanError>
41:     │
42:     ├── batch classification (BatchFamily::Query | Mutation | Single)
43:     │     └── contiguous same-collection ops → execute_query_batch / execute_update_batch
44:     │
45:     ├── individual dispatch → to_rest_route() → Route → client.execute_route()
46:     │                                                  └── REST: serialized JSON
47:     │                                                  └── gRPC: execute_grpc_route() typed protobuf conversion
48:     │
49:     └── response normalization (ExecResponse)
50: ```
51: 
52: Canonical plan is `PlannedOperation` (transport-neutral). `Route { method, path, query, body }` is the **REST projection** of a plan, not the source of truth. Semantic types (`PlanQueryInput`, `PlanPointId`, `PlanVectorValue`) remain typed until a transport boundary. gRPC converts typed plan structs directly to protobuf via `to_query_points`, `to_vector_input`, `plan_vector_to_proto`, etc. — no JSON intermediary for query vectors or point IDs. Formula lowering still emits `serde_json::Value` (lower_formula_expr → to_formula_expression round-trips through JSON).
53: 
54: ### Crate Division Boundaries
55: 
56: * **`qql-core`**: The parser, lexer, typed AST (`QueryExpr` enum, `FilterExpr`, `ComparisonOp`, etc.), AST transforms (`inject_filter`), and explain formatting. Performs NO network or file I/O. Has NO knowledge of Qdrant endpoints, REST JSON shapes, or transport protocols. Features: `default = []`, `serde`, `json`, `std`. Uses owned `String` types throughout — no lifetime parameters on input.
57: 
58: * **`qql-plan`**: Transport-neutral lowering layer. Contains the fallible planner `plan()` returning `PlannedOperation`, typed filter/query/mutation/DDL/embedding types (`PlanPointId`, `PlanVectorValue`, `PlanQueryInput`), and `to_rest_route()` for the REST projection. `Route` and `RequestBody` are REST-specific. Depends ONLY on `qql-core`. No networking, no tokio, no reqwest.
59: 
60: * **`qql-embed`**: Shared embedding layer. `Embedder` trait (`embed_dense` / `embed_sparse` / `embed_multi`), local sparse BM25, `resolve_query_vector_kinds` (schema topology → dense/sparse/multi flags), and `resolve_embeddings` (TEXT → Dense | Sparse | MultiDense). Unknown `USING` kinds fail closed (`QQL-VECTOR-KIND`). No Qdrant I/O. Used by runtime (`HttpEmbedder`), edge (`FastEmbedder`), and wasm (fetch/JS adapters).
61: 
62: * **`qql-runtime`**: The executor and transport adapters. Package name is `qql`. The `Executor` holds a `Box<dyn QdrantOps>` (single unified trait with 11 methods) and optional `Embedder`. Calls `prepare_statement` (**schema vector resolution first**, then embeddings, then upsert schema prep) → `plan()` → batch classification / dispatch. DDL flows through `plan()` → REST projection → `execute_route()` or `execute_grpc_route()`. Features: `default = ["grpc", "rest"]`, `grpc`, `rest`. Re-exports embed API via `qql::embedder` / `qql::sparse`.
63: 
64: * **`qql-edge`**: In-process vector search using qdrant-edge + optional fastembed-rs. Zero network. Implements `QdrantOps` with batch methods fanning out to individual routes (no native edge batch RPC). Uses `qdrant-edge` 0.7.x.
65: 
66: * **`qql-cli`**: CLI binary. Uses the executor via REST/adapter construction.
67: 
68: * **Foreign Bindings**: PyO3 (`pyqql`), N-API (`nqql`), Wasm-bindgen (`qql-wasm`). Expose parser, tokenization, filter injection, explain, `compile_query` (via `routing::route()`), and `Client` classes. Keep public class names (`Client`, `HttpEmbedder`, `Stmt`), return shapes, and error mappings aligned.
69: 
70: ### Permanently Removed Abstractions
71: 
72: The following old abstractions have been permanently removed — do NOT reintroduce them:
73: 
74: - `offline.rs` / `CompiledQuery` — replaced by `qql_plan::plan::plan()` + `PlannedOperation`
75: - `filter_conv/` — replaced by `qql_plan::filter::lower_filter()`
76: - `pipeline/` module — replaced by `qql_plan::types`
77: - `QdrantCoreOps` / `QdrantAdminOps` dual-trait — merged into single `QdrantOps`
78: - `QueryMode`, `QueryType`, `SearchWith`, `SelectStmt` — replaced by `QueryExpr` enum (12 variants)
79: - `qdrant-client` crate dependency — replaced by raw `tonic` 0.14
80: - `SELECT` / `INSERT INTO` keywords — replaced by `QUERY POINTS` / `UPSERT INTO`
81: - String filter operators (`"="`, `">"`, etc.) — replaced by `ComparisonOp` enum
82: - `Token::pos` — replaced by `Token::span: Span { start, end }`
83: - `QqlError::runtime()` — replaced by `QqlError::execution(code, message, span)`
84: - `QqlError::syntax()` — replaced by `QqlError::parse(code, message, span)`
85: - `executor/ddl.rs` — DDL now flows through `qql_plan::plan` → REST projection / gRPC route
86: - `CompiledQuery` / `offline.rs` — eliminated; `routing::route()` is a compatibility wrapper around `plan()` + `to_rest_route()`
87: 
88: ### Current QueryExpr Variants (12 total)
89: 
90: ```
91: Points, Nearest, Recommend, Context, Discover, OrderBy,
92: SampleRandom, Fusion, Formula, RelevanceFeedback, Hybrid, Rerank
93: ```
94: 
95: ### Error Model
96: 
97: ```rust
98: pub enum ErrorKind { Lex, Parse, Validation, Execution, Transport, Backend }
99: pub struct QqlError { kind: ErrorKind, code: &'static str, message: String, span: Option<Span> }
100: pub struct Span { start: usize, end: usize }
101: ```
102: 
103: Error kind is explicit — never inferred from position. No `runtime` constructor.
104: 
105: ### QdrantOps Trait (11 methods)
106: 
107: ```rust
108: pub trait QdrantOps: Send + Sync {
109:     // DDL / metadata
110:     async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
111:     async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
112:     async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
113:     async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
114:     async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
115:     async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
116:     async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
117:     async fn delete_field_index(&self, collection_name: &str, field_name: &str) -> Result<(), QqlError>;
118: 
119:     // DML via REST route projection
120:     async fn execute_route(&self, route: Route) -> Result<serde_json::Value, QqlError>;
121: 
122:     // Batch methods (added after original 9)
123:     async fn execute_query_batch(&self, collection: &str, batch: &QueryBatchRequest) -> Result<Vec<serde_json::Value>, QqlError>;
124:     async fn execute_update_batch(&self, collection: &str, batch: &UpdateBatchRequest) -> Result<Vec<serde_json::Value>, QqlError>;
125: }
126: ```
127: 
128: Three implementations: `RestQdrant`, `GrpcQdrant`, `EdgeQdrant`. The gRPC adapter bypasses `execute_route` for DML — it uses `execute_grpc_route()` which converts typed `RequestBody` variants directly to protobuf. For REST, `execute_route` serializes `RequestBody` as JSON.
129: 
130: ### Statement → Endpoint Matrix (18 routes)
131: 
132: | QQL Statement | Endpoint | Method |
133: |---|---|---|
134: | `QUERY ...` (search) | `/points/query` | POST |
135: | `QUERY ... GROUP BY` | `/points/query/groups` | POST |
136: | `QUERY POINTS (ids)` | `/points` | POST |
137: | `SCROLL ...` | `/points/scroll` | POST |
138: | `COUNT ...` | `/points/count` | POST |
139: | `UPSERT ...` | `/points` | PUT |
140: | `DELETE ...` | `/points/delete` | POST |
141: | `CLEAR PAYLOAD ...` | `/points/payload/clear` | POST |
142: | `DELETE VECTOR ...` | `/points/vectors/delete` | POST |
143: | `UPDATE ... VECTOR` | `/points/vectors` | PUT |
144: | `UPDATE ... PAYLOAD` | `/points/payload` | POST |
145: | `CREATE COLLECTION` | `/collections/{c}` | PUT |
146: | `ALTER COLLECTION` | `/collections/{c}` | PATCH |
147: | `DROP COLLECTION` | `/collections/{c}` | DELETE |
148: | `CREATE INDEX` | `/collections/{c}/index` | PUT |
149: | `DROP INDEX` | `/collections/{c}/index/{field}` | DELETE |
150: | `SHOW COLLECTIONS` | `/collections` | GET |
151: | `SHOW COLLECTION` | `/collections/{c}` | GET |
152: 
153: ### gRPC Stack
154: 
155: - `qdrant-client` dropped entirely — replaced with `tonic` 0.14 + `tonic-prost` + `tonic-prost-build`
156: - Proto files in `proto/`, compiled at build time via `tonic-prost-build`
157: - `GrpcQdrant` wraps `tonic::Channel` with `connect_lazy`
158: - `grpc_route.rs` (~2,637 lines) converts typed qql-plan structs → generated protobuf types directly for query vectors, point IDs, and vector values. DDL sub-configs still read from `serde_json::Value` fields (hnsw_config, optimizers_config, quantization_config). Formula expressions still round-trip through JSON via `lower_formula_expr` → `to_formula_expression`.
159: - `grpc.rs` (~513 lines) is the thin Tonic client wrapper; heavy conversion lives in `grpc_route.rs`
160: - Tonic features: `channel`, `codegen`, `tls-ring`, `tls-webpki-roots` (no server, no axum, no router)
161: - API key support via `ApiKeyInterceptor` (RUN-009 fixed)
162: - DDL routes (CreateCollection, UpdateCollection, CreateIndex, DropIndex, DeleteCollection, shard operations) all handled in `execute_grpc_route`
163: 
164: ### Serialization Policy
165: 
166: - `qql-core`: Serde optional (`default = []`, features `serde` and `json` separately). Parser-only consumers pay for nothing.
167: - `qql-plan`: Always depends on serde/serde_json — builds JSON wire bodies matching OpenAPI format exactly. Typed semantic primitives (`PlanPointId`, `PlanVectorValue`, `PlanQueryInput`) implement `Serialize` directly.
168: - `qql-runtime`: Uses serde/serde_json in REST adapter. gRPC adapter uses typed protobuf conversion.
169: - Bindings: All enable `qql-core/serde` + `qql-core/json` for AST serialization and `Value::from_json()`.
170: 
171: ---
172: 
173: ## 2. OpenAPI Schema Contract Testing
174: 
175: All generated route payloads are validated directly against Qdrant's official
176: `crates/qql-runtime/openapi.json` specification in
177: `crates/qql-runtime/src/contract_test.rs`:
178: 
179: 1. **`Query` Schema Validation**: All 12 query expression variants are validated against `# /components/schemas/Query`.
180: 2. **`Filter` Schema Validation**: All 17 filter expression variants are validated against `# /components/schemas/Filter`.
181: 3. **`PointRequest` & `ScrollRequest` Validation**: Validated against `# /components/schemas/PointRequest` and `# /components/schemas/ScrollRequest`.
182: 
183: REST/gRPC operation-matrix coverage is not yet complete — gRPC tests validate dispatch does not error but do not assert field-level parity for formulas, shard keys, search params, or DDL options (QUALITY-003).
184: 
185: ---
186: 
187: ## 3. Minimalist Code Design Philosophy
188: 
189: 1. **Size Constraints**: Target <400 lines per file where possible. Split large files into modules.
190: 2. **Error Propagation**: Dispatch directly; bubble up downstream errors. No pre-emptive checks.
191: 3. **No JSON-as-IR**: `RequestBody` is typed. JSON only at the REST boundary, except for DDL sub-configs and formula expressions which still use JSON within gRPC conversion.
192: 4. **No duplicate planners**: `qql_plan::plan::plan()` is the single fallible planner. `routing::route()` is a compatibility wrapper. DDL goes through the same planner.
193: 5. **No glue code**: Each layer has one responsibility. No wrappers around wrappers.
194: 
195: ---
196: 
197: ## 4. AST Query Transformation & Filter Injection
198: 
199: ```rust
200: pub fn inject_filter(
201:     statement: &mut Stmt,
202:     field: &str,
203:     operator: ComparisonOp,   // typed enum (Eq, Gt, Gte, Lt, Lte)
204:     value: Value,             // owned, no lifetime
205: ) -> Result<(), QqlError>
206: ```
207: 
208: Recursively injects into QueryStmt (including all CTEs and prefetches), Scroll, Count, Delete,
209: UpdatePayload, and Upsert (when `operator == Eq` and `field != "id"`, injects into point
210: payloads). Callers must convert their string operators before calling.
211: 
212: ---
213: 
214: ## 5. Grammar and Runtime Invariants
215: 
216: * Parsing is strict: malformed clauses return `QqlError::Parse`, never silently keep defaults.
217: * `Span { start, end }` uses byte offsets. `Token::pos` is `pub(crate)`; public API uses `span`.
218: * Script splitting requires semicolons between statements. `parse_all()` rejects adjacent unseparated statements.
219: * `SELECT` is rejected as an unrecognized statement. Use `QUERY POINTS` for point retrieval.
220: * Duplicate object keys, config keys, CTE names, and query clauses are rejected.
221: * `QqlError` always carries an explicit `ErrorKind` and `Span`.
222: * `SHARD '<key>'` routing is supported on QUERY, COUNT, UPSERT, SCROLL, and DELETE for custom-sharded collections.
223: * Collection creation supports `shard_number`, `sharding_method`, and `shard_keys` via `WITH PARAMS`.
224: * Payload indexes support `is_tenant = true` for Qdrant-native tenant optimization.
225: 
226: ---
227: 
228: ## 6. Host Language SDK Reference Manuals
229: 
230: Dedicated reference guides for each host SDK live under `skills/qql-skill/references/`:
231: 
232: - **[`qql-examples.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/qql-examples.md)**: Pure QQL query examples (` ```sql ` code blocks strictly).
233: - **[`python-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/python-sdk.md)**: Python `pyqql` PyO3 client and AST functions.
234: - **[`node-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/node-sdk.md)**: Node.js `nqql` N-API client and `parseJson` usage.
235: - **[`wasm-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/wasm-sdk.md)**: WebAssembly `qql-wasm` browser & edge client.
236: - **[`rust-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/rust-sdk.md)**: Native Rust `qql` runtime & `qql-core` SDK reference.
237: - **[`qql-multitenancy.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/qql-multitenancy.md)**: Complete multi-tenant guide: shard routing, filter injection, `is_tenant` indexing.
238: 
239: ---
240: 
241: ## 7. Developer Workflow
242: 
243: ### Testing
244: ```bash
245: PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --workspace --all-targets
246: ```
247: 
248: ### Formatting & Clippy
249: ```bash
250: cargo fmt --check
251: PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo check --workspace --all-targets
252: PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo clippy --workspace --all-targets -- -D warnings
253: ```
254: 
255: ### Known Workspace Blockers
256: - `qql-wasm`: `async_trait(?Send)` on WASM Embedder impl conflicts with host `+ Send` trait bound — only builds for `wasm32-unknown-unknown` target.
257: - `pyqql`: PyO3 0.23.5 maximum Python version is 3.13; host has Python 3.14. Set `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` to bypass.
258: - `qql-edge`: Requires fastembed-rs with specific native dependencies.
259: 
260: ### Token Definition Hygiene
261: When adding a new keyword token to `token.rs`:
262: 1. Add the variant to `pub enum TokenKind`.
263: 2. Add a `Variant => "STRING"` entry to `gen_as_str!`.
264: 3. Add a `"STRING" => TokenKind::Variant` entry to `gen_keywords!`.
265: 
266: ### Workspace Hygiene
267: * Keep workspace version in root `Cargo.toml` as single source of truth.
268: * Minimize dependency surface. Check unused deps with `cargo +nightly udeps`.
269: * Inspect `git status` before making changes; don't overwrite unrelated work.

(End of file - total 269 lines)
</content>
```

**Tool: read**

**Input:**
```json
{
  "filePath": "/data/codebases/qql-rs/gaps.md"
}
```

**Output:**
```
<path>/data/codebases/qql-rs/gaps.md</path>
<type>file</type>
<content>
1: # QQL Implementation Gaps
2: 
3: Engineering source of truth. Skill-facing summary:
4: [`skills/qql-skill/references/qql-gaps.md`](skills/qql-skill/references/qql-gaps.md).
5: 
6: **Spec authority for wire shapes:**
7: [`crates/qql-runtime/openapi.json`](crates/qql-runtime/openapi.json) and
8: [`crates/qql-runtime/proto/`](crates/qql-runtime/proto/). Do not invent fields
9: not present there.
10: 
11: Status: **OPEN** | **PARTIAL** | **DONE** | **BLOCKED**
12: 
13: ---
14: 
15: ## 0. Edge capability matrix
16: 
17: | Capability | Edge status | Notes |
18: |---|---|---|
19: | Dense ANN + sparse + hybrid FUSION | **Yes** | Default offline path |
20: | Multivector store + query | **Yes** | Opt-in multi model for embed |
21: | `CROSS RERANK` / CLIP IMAGE | **Opt-in** | Models required; IMAGE = local path |
22: | `GROUP BY` | **No** | `QQL-EDGE-UNSUPPORTED-GROUP-BY` |
23: | `SHARD` / ACORN / ALTER | **No** | Stable `QQL-EDGE-UNSUPPORTED-*` catalog |
24: | `timeout` / `consistency` | **N/A** | Single-node; request-level opts ignored |
25: | Batch RPC | **No** | Fan-out only (QUAL-002 deferred) |
26: 
27: ---
28: 
29: ## 1. Multivector / CLIP / cross-encoder
30: 
31: | ID | Status | Item |
32: |---|---|---|
33: | GAP-MV / KIND | **DONE** | Host multi/image/rerank, schema-first kinds |
34: | GAP-EDGE-001 / UX-008 | **DONE** | CLI doctor host summary |
35: | GAP-EDGE-002 | **PARTIAL** | IMAGE = local path only |
36: | GAP-EDGE-003 | **PARTIAL** | Heavy model e2e still optional/CI-skippable |
37: | GAP-EDGE-004 | **OPEN** | Edge GROUP BY (engine); catalog + docs closed product decision |
38: 
39: ---
40: 
41: ## 2. UX / language
42: 
43: | ID | Status | Notes |
44: |---|---|---|
45: | **GAP-UX-001** | **BLOCKED** | Group OFFSET — not in Qdrant OpenAPI; fail-closed |
46: | **GAP-UX-002** | **PARTIAL** | MMR dense-only; fail-closed sparse |
47: | **GAP-UX-003** | **DONE** | `PARAMS (timeout, consistency)` OpenAPI/proto aligned |
48: | **GAP-UX-004** | **DONE** | `USING HYBRID` |
49: | **GAP-UX-005** | **DONE** | ACORN + max_selectivity |
50: | **GAP-UX-006** | **DONE** | `inject_shard_key` |
51: | **GAP-UX-007** | **DONE** | Edge GROUP BY: clear unsupported catalog + remote hint |
52: | **GAP-UX-008** | **DONE** | Doctor hosts |
53: 
54: ---
55: 
56: ## 3. Quality / transport
57: 
58: | ID | Status | Item |
59: |---|---|---|
60: | **GAP-QUAL-001** | **DONE** | Query path parity + **DDL sub-config REST OpenAPI projection**: flatten create params, nest quantization (`scalar`/`product`/`binary`/`turbo`), nest index `field_schema`, multi-step create (deferred fan-out params + shard keys) matching gRPC. gRPC accepts flat IR and nested OpenAPI quant shapes. |
61: | **GAP-QUAL-002** | **OPEN** | Edge batch fan-out (deferred; check fastembed batch later) |
62: | **GAP-QUAL-003** | **DONE** | Edge unsupported catalog in `qql-edge/src/backend/unsupported.rs` with stable codes + remote remediation |
63: | **GAP-QUAL-004** | **DONE** | OpenAPI contract cases for multi-dense, hybrid, formula, groups, timeout query params; CROSS RERANK is plan-only (not a fake Query body); multi REST↔gRPC shape tests |
64: 
65: ---
66: 
67: ## 4. Fix order (remaining)
68: 
69: 1. **GAP-QUAL-002** — Edge / host batch (after fastembed batching review)  
70: 2. **GAP-EDGE-002** — Optional IMAGE URL fetch  
71: 3. **GAP-EDGE-003** — Optional heavy multi+CROSS e2e  
72: 4. **GAP-UX-001** — Only when Qdrant groups support pagination  
73: 
74: 
75: ---
76: 
77: ## 5. Supported (do not re-open)
78: 
79: - Hybrid `USING HYBRID` / `QUERY HYBRID`
80: - Request timeout + read consistency
81: - ACORN + max_selectivity (remote)
82: - `inject_shard_key`
83: - Multi / CLIP / CROSS RERANK when hosts configured
84: - Fail-closed group OFFSET + MMR sparse
85: - Edge unsupported catalog + REST/gRPC query parity tests
86: 
87: ---
88: 
89: ## 6. Related docs
90: 
91: | Doc | Role |
92: |---|---|
93: | OpenAPI / proto | Wire truth |
94: | `docs/syntax.md` | Language |
95: | `crates/qql-edge/README.md` | Edge unsupported table |
96: | Skill `qql-gaps.md` | Agent rules |

(End of file - total 96 lines)
</content>
```

---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 215.5s)

_Thinking:_

Let me now explore the key crates in depth. I'll use subagents to parallelize the deep dive into different parts of the codebase.

Now let me launch parallel deep-dive explorations into the key crates and contract files:

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive qql-core crate",
  "subagent_type": "rust-explore",
  "prompt": "Explore the qql-core crate at /data/codebases/qql-rs/crates/qql-core thoroughly. Find:\n1. Dead code / unused types / unused functions\n2. Half-baked features or incomplete implementations\n3. Documentation issues (missing docs, stale comments referencing removed features like SELECT/old abstractions)\n4. Architecture issues (code that should be elsewhere, broken abstractions)\n5. Any code that mentions Qdrant API specifics that should be in qql-runtime or qql-plan instead\n6. TODO/FIXME/HACK comments\n7. Check Cargo.toml for unused dependencies\n\nRead the Cargo.toml, src/lib.rs, and explore all source files. Report file names, line numbers, and specific issues found."
}
```

**Output:**
```
<task id="ses_0523a700fffeSfCsZ9wWJR8pBc" state="completed">
<task_result>
Here is the comprehensive report.

---

## 📋 qql-core Crate Audit Report

### 1. Dead Code / Unused Types & Functions

**1a. `parse_optional_vector_name()` — explicitly marked dead**
- **File:** `src/parser/helpers.rs:28`
- **Line:** `#[allow(dead_code)]`
- **Detail:** This pub method is never called anywhere. It was probably intended for optional vector name parsing but never wired up.

**1b. Entire `syntax.rs` pest grammar module — dead code in production path**
- **File:** `src/parser/syntax.rs:1-63`
- **Line 1:** `#![allow(dead_code)]` at the module level
- **Detail:** The functions `validate_statement` (line 11) and `validate_script` (line 17) are **never called** from the main parsing pipeline. `AstLowerer` does its own parsing without any pest-based validation frontend. The generated pest grammar (`grammar/qql.generated.pest`) and the `CanonicalSyntaxParser` struct are compiled but completely unused outside of two test functions (lines 40-62). This means `pest` and `pest_derive` dependencies exist solely for unused grammar validation.
- **Impact:** The `pest` + `pest_derive` dependencies in `Cargo.toml` are **effectively unused** in production builds.

**1c. Dead keyword entries in `gen_keywords!` map**
- **File:** `src/token.rs:382`
- **Line:** `"STAR" => TokenKind::Star,`
- **Detail:** The keyword `"STAR"` is registered but `*` is handled by the lexer's `single_char` path (lexer.rs:37), never via the identifier/keyword lookup path (`lookup_keyword`). Any user typing `STAR` as an identifier would get it resolved to `TokenKind::Star`, but no parser rule expects `TokenKind::Star` as a keyword — only as the `*` multiplication operator in formulas. Dead entry.
- **Also:** `"DEFAULT"` is not registered as a keyword — it's instead handled as a bare word in `ascii_equal` checks (formula.rs:155), which is inconsistent with how other bare words are handled. But "DEFAULT" isn't in the keyword map, which may be intentional.

**1d. `TokenKind::Semicolon` — keyword map entry never hit**
- **File:** `src/token.rs:156` (enum declaration), `src/token.rs:283` (as_str)
- **Detail:** `Semicolon` is not in `gen_keywords!` — correct, since `;` is a single-char token. No issue here.

---

### 2. Half-Baked Features / Incomplete Implementations

**2a. Pest grammar validation is disconnected from parsing**
- **File:** `src/parser/syntax.rs` (entire file)
- **Detail:** The pest grammar exists, compiles, and has tests, but the `validate_statement`/`validate_script` functions are **never invoked** by `AstLowerer::lower_statement` or `AstLowerer::lower_script`. The parser has two independent parsing mechanisms (pest PEG grammar + manual token-stream `AstLowerer`), but the pest one is not used. This suggests an incomplete migration or dead-end prototype.

**2b. `grammar/qql.generated.pest` references external code generator**
- **File:** `grammar/qql.generated.pest:1-4`
- **Lines 1-4:** `// @generated by qql-grammar-gen. DO NOT EDIT. // Source: language/v1/grammar.pest`
- **Detail:** The grammar file references a `language/v1/grammar.pest` source outside this crate and a `qql-grammar-gen` tool. Neither exists within the `qql-core` crate or its workspace as a dependency, suggesting these are from a different crate or external tooling that may not be kept in sync.

**2c. `CollectionMode::Rerank` variant with no rerank-specific fields**
- **File:** `src/ast/statement.rs:572-573`
- **Detail:** The `Rerank` variant (as opposed to `Hybrid` and `Dense`) carries **no model or vector fields** — it's a unit-like variant. This seems incomplete since rerank-oriented collections would need to specify a reranker model.

**2d. `is_contextual_field_name` vs `is_contextual_identifier` discrepancy**
- **File:** `src/parser/mod.rs:64-99`
- **Detail:** `is_contextual_field_name` includes `TokenKind::By` (line 76), but `is_contextual_identifier` does **not** include `TokenKind::By` (lines 83-99). These two functions list nearly identical token sets, but the omission of `By` from `is_contextual_identifier` is suspicious. Either `By` should be in both or neither — this asymmetry looks like a bug or an oversight.

---

### 3. Documentation Issues

**3a. Stale comments mentioning old SELECT/INSERT/BOOST syntax**
- **File:** `src/parser/syntax.rs:47-55`
- **Lines 47-55:** Test cases for `"SELECT * FROM docs"`, `"INSERT INTO docs VALUES {id: 1}"`, `"BOOST ($score * 2)"`
- These are in a test named `canonical_grammar_rejects_legacy_statements`. While accurate, the comments don't explain that these are **former** syntax forms.

- **File:** `src/tests/parser_tests.rs:449-457`
- **Line 449:** `fn removed_pre_v1_aliases_are_rejected()`
- Good test naming, but the test comments at lines 451-452 just list the strings with no doc comment explaining this is a retained test for rejection behavior.

- **File:** `src/tests/mod.rs:15`
- **Line 15:** `let parse = Parser::parse("SELECT").expect_err("SELECT is not a statement");`
- Fine as error message, but the string `"SELECT is not a statement"` is a bit misleading — it reads like an assertion message, not doc.

**3b. Stale pest grammar comment**
- **File:** `src/parser/mod.rs:29-31`
- **Lines 29-31:** `/// Syntax acceptance is generated from `language/v1/grammar.pest`. The /// private [`AstLowerer`] converts accepted source into the typed QQL AST.`
- **Problem:** This comment claims syntax acceptance is "generated from `language/v1/grammar.pest`", but the actual parsing is done by the manual `AstLowerer`, not by the pest grammar. The pest grammar is not used for acceptance. This is misleading documentation.

---

### 4. Architecture Issues

**4a. Qdrant API specifics leaking into core crate (major issue)**
- The following files contain Qdrant API-level details (OpenAPI, proto, HTTP, gRPC, transport) that should be in `qql-runtime` or `qql-plan`:
  - `src/ast/statement.rs:210` — "Not sent to Qdrant as MaxSim"
  - `src/ast/statement.rs:230-234` — "Read consistency for Qdrant point reads" with OpenAPI/proto/REST/gRPC details
  - `src/ast/statement.rs:260-263` — timeout is "OpenAPI query param / proto field"
  - `src/ast/transform.rs:46-48` — "Qdrant accepts `shard_key` on query/mutation bodies (OpenAPI / proto)"
  - `src/parser/with_clause.rs:28` — "// OpenAPI query param / proto field — seconds, minimum 1."
  - `src/parser/with_clause.rs:30` — "// OpenAPI ReadConsistency: factor N or majority|quorum|all."
  - `src/parser/with_clause.rs:250` — "/// OpenAPI `ReadConsistency`..."
  - `src/error.rs:35` — "Transport-layer error (HTTP/gRPC connectivity, timeout)"
  - `src/error.rs:37` — "Qdrant backend error"

**4b. `SearchParams` mixes search parameters with request metadata**
- **File:** `src/ast/statement.rs:248-265`
- **Detail:** `SearchParams` contains both actual search parameters (hnsw_ef, exact, quantization, rrf_k, etc.) and request-level metadata (timeout, consistency). These are architecturally different concerns — timeout and consistency are transport/request-level, while the rest are search-level. They should be separated, with the transport fields living in `qql-runtime`.

**4c. `ReadConsistency` enum defined in core crate**
- **File:** `src/ast/statement.rs:237-246`
- **Detail:** `ReadConsistency` is a Qdrant-specific transport concept (how many replicas to read from). Defining this in `qql-core` couples the AST to the Qdrant API. It should be in `qql-runtime`.

**4d. `QuantizationSearchParams` defined in core but is a Qdrant concept**
- **File:** `src/ast/statement.rs:224-228`
- **Detail:** `QuantizationSearchParams` (ignore, rescore, oversampling) is a Qdrant search runtime parameter with no place in a pure AST.

**4e. `Self::Dict` branch logic in `point_vectors_from_value` is fragile**
- **File:** `src/parser/upsert.rs:141-153`
- **Detail:** The code checks if a `Value::Dict` contains `indices` or `values` keys — if it does, it's treated as a sparse vector; if not, it's treated as named vectors. This means a user **cannot** have a named vector whose name happens to be "indices" or "values". This is a latent bug that could confuse users.

---

### 5. Code Mentioning Qdrant API Specifics (Should Be in qql-runtime / qql-plan)

| File | Line(s) | Content |
|------|---------|---------|
| `src/ast/statement.rs` | 210 | "Not sent to Qdrant as MaxSim — executor scores client-side" |
| `src/ast/statement.rs` | 230-234 | "Read consistency for Qdrant point reads... OpenAPI `ReadConsistency` / proto... REST: query param... gRPC: read_consistency" |
| `src/ast/statement.rs` | 260 | "Request-level timeout in **seconds** (OpenAPI query param / proto field)" |
| `src/ast/statement.rs` | 263 | "Request-level read consistency (OpenAPI query param / proto field)" |
| `src/ast/transform.rs` | 46-48 | "Qdrant accepts `shard_key` on query/mutation bodies (OpenAPI / proto)" |
| `src/parser/with_clause.rs` | 28 | "// OpenAPI query param / proto field — seconds, minimum 1." |
| `src/parser/with_clause.rs` | 30 | "// OpenAPI ReadConsistency: factor N or majority\|quorum\|all." |
| `src/parser/with_clause.rs` | 250 | "/// OpenAPI `ReadConsistency`: integer factor or majority\|quorum\|all." |
| `src/error.rs` | 35 | "Transport-layer error (HTTP/gRPC connectivity, timeout)" |
| `src/error.rs` | 37 | "Qdrant backend error (non-success response, malformed response)" |

---

### 6. TODO / FIXME / HACK Comments

**No TODO, FIXME, or HACK comments found.** The grep returned zero results across all `.rs` files in the crate.

---

### 7. Cargo.toml — Unused Dependencies

**7a. `pest` and `pest_derive` — likely unused in production**
- **File:** `Cargo.toml:23-24`
- **Detail:** `pest` and `pest_derive` are only used in `src/parser/syntax.rs`, which is a dead-code module (`#![allow(dead_code)]`) whose functions are never called from the main parsing path. The pest grammar parser is never invoked during actual parsing. These dependencies exist only for the unused grammar validation tests.
- **Recommendation:** Either remove `pest`/`pest_derive` or integrate the pest validation into the actual parsing pipeline and remove the `#![allow(dead_code)]`.

**7b. `phf` — used, no issue**
- **File:** `Cargo.toml:25`
- **Detail:** `phf` is used in `src/token.rs:25` for the `KEYWORDS` static map. Valid use.

**7c. `serde` / `serde_json` — optional, used conditionally**
- **File:** `Cargo.toml:26-27`
- **Detail:** Used for `#[cfg_attr(feature = "serde", derive(...))]` and `#[cfg(feature = "json")]` blocks. Valid use.

---

### Summary of Issues by Severity

| Severity | Issue | Location |
|----------|-------|----------|
| **High** | `pest` + `pest_derive` dependencies for dead code module | `Cargo.toml` + `src/parser/syntax.rs:1-63` |
| **High** | Qdrant API specifics (OpenAPI, proto, gRPC, HTTP) in core AST | Multiple files (see §5) |
| **Medium** | `is_contextual_field_name` vs `is_contextual_identifier` asymmetry (`By` missing) | `src/parser/mod.rs:64-99` |
| **Medium** | `SearchParams` mixes search params with request metadata | `src/ast/statement.rs:248-265` |
| **Medium** | `ReadConsistency` is a Qdrant transport concept in core AST | `src/ast/statement.rs:237-246` |
| **Medium** | Stale doc comment claiming pest grammar generates acceptance | `src/parser/mod.rs:29-31` |
| **Low** | `parse_optional_vector_name()` marked `#[allow(dead_code)]` | `src/parser/helpers.rs:27-28` |
| **Low** | `"STAR"` dead entry in keyword map | `src/token.rs:382` |
| **Low** | Named vectors cannot have keys "indices" or "values" | `src/parser/upsert.rs:141-144` |
| **Low** | `CollectionMode::Rerank` has no rerank-specific fields | `src/ast/statement.rs:572` |
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive qql-plan crate",
  "subagent_type": "rust-explore",
  "prompt": "Explore the qql-plan crate at /data/codebases/qql-rs/crates/qql-plan thoroughly. Find:\n1. Dead code / unused types / unused functions / unused enum variants\n2. Half-baked features or incomplete implementations\n3. Documentation issues (missing docs, stale comments)\n4. Architecture issues - check if plan types match openapi.json contract properly\n5. Any code that references removed abstractions (like SELECT, old QueryMode, etc.)\n6. TODO/FIXME/HACK comments\n7. Check Cargo.toml for unused dependencies\n8. Check src/lib.rs and all module files\n\nReport file names, line numbers, and specific issues found."
}
```

**Output:**
```
<task id="ses_0523a6692ffe9jPjIxsfp0mQdV" state="completed">
<task_result>
Now I have all the information needed. Here is the comprehensive report:

---

## Comprehensive Analysis of `qql-plan` Crate

### 1. Dead Code / Unused Items

#### 1a. `Method::as_str()` — dead method
- **File**: `src/types.rs:20-29`
- **Issue**: The `pub fn as_str(&self) -> &'static str` method on `Method` is never called anywhere in the entire codebase. Method is used for equality checks (`== Method::Post`, etc.) but `as_str()` is dead.
- **Confidence**: confirmed

#### 1b. `MinShould` struct — dead type
- **File**: `src/types.rs:117-120`
- **Issue**: `pub struct MinShould { pub conditions: Vec<FilterClause>, pub min_count: u64 }` is defined but never referenced anywhere — not in filter lowering, not in the runtime, not in tests.
- **Confidence**: confirmed

#### 1c. `embedding_has_wait` — dead function
- **File**: `src/mutation.rs:160-168`
- **Issue**: `pub fn embedding_has_wait(spec: &EmbeddingSpec) -> bool` is only used within its own function (`mutation.rs:167`) via the `Multi` recursive call. Never called from outside the function. It's a dead entry point.
- **Confidence**: confirmed

#### 1d. `lower_point_vectors` — dead function
- **File**: `src/mutation.rs:35-37`
- **Issue**: `pub fn lower_point_vectors(vectors: &PointVectors) -> PlanPointVectors` is defined but never called anywhere (not internally, not externally).
- **Confidence**: confirmed

#### 1e. `MatchValue::TextAny` and `MatchValue::Except` — dead enum variants
- **File**: `src/types.rs:168-176`
- **Issue**: These two variants of the `MatchValue` enum are never constructed anywhere in the codebase. They are only matched in `grpc_route.rs` for the runtime, but the plan layer never creates them. No filter lowering produces `TextAny` or `Except`.
- **Confidence**: confirmed

#### 1f. The entire `embedding` module is effectively dead code externally
- **File**: `src/embedding.rs` (entire file, 345 lines)
- **Issue**: The `pub mod embedding` module is declared in `lib.rs` and `extract_jobs()` is `pub`, but **no external crate calls it**. All matches in the repo for `extract_jobs` are internal tests within `embedding.rs` itself. The types `EmbeddingJob`, `EmbeddingKind`, `EmbeddingDestination` are likewise unused externally. This appears to be a module whose consumer was split out (perhaps `qql-embed`) but the old extraction logic was left behind.
- **Confidence**: confirmed

#### 1g. Dev-dependencies duplicate regular dependencies
- **File**: `Cargo.toml:24-26`
- **Issue**: `qql-core` and `serde_json` are listed in both `[dependencies]` and `[dev-dependencies]` with identical specs. The `[dev-dependencies]` entries are redundant — Rust/Cargo already makes regular deps available to tests.
- **Confidence**: confirmed

---

### 2. Half-Baked Features / Incomplete Implementations

#### 2a. `CrossRerank` REST route is a stub
- **File**: `src/plan.rs:900-906`
- **Issue**: The `to_rest_route` arm for `PlannedOperation::CrossRerank` returns a `Route` with:
  - `method: Method::Post`
  - `path: "/collections/{collection}/points/query"`
  - `body: None` (empty!)
  
  The comment says "CrossRerank is client-side; never projected as a single Qdrant route." But it returns a POST route to the query endpoint with **no body** — an invalid HTTP request. If any code calls `to_rest_route` on a `CrossRerank` operation, it will produce a broken route. Either this arm should panic with a clear message, or the function should return `Result<Route, ...>`.
- **Confidence**: confirmed

#### 2b. `from` impl for `PlanQueryInput` converts `Image` to `Document` losing semantics
- **File**: `src/semantic.rs:117-121`
- **Issue**: `QueryInput::Image { source, model }` is converted to `PlanQueryInput::Document { text: source, model }`. The `source` field of an image (likely a URL or file path) is treated as document text. This is a lossy conversion that conflates image sources with text content — the distinction is lost at the plan layer.
- **Confidence**: confirmed

#### 2c. `FilterCompound::shard_key` field is always set to `None`
- **File**: `src/filter.rs:9-10,15-16,21-22,34-35,119-120,125-126,131-132`
- **Issue**: Every place that constructs `FilterCompound` passes `shard_key: None`. The field exists in the struct (`types.rs:72`) but is never populated. Either it's a dead field or the feature to use shard_key at the filter level is incomplete.
- **Confidence**: confirmed

#### 2d. Missing `shard_key` on mutation request types
- **File**: `src/types.rs:617-652`
- **Issue**: `UpdateVectorRequest`, `UpdatePayloadRequest`, `ClearPayloadRequest`, `DeleteVectorRequest` all lack a `shard_key` field, yet `DeleteRequest` and `UpsertRequest` have one. The Qdrant API supports `shard_key` on these endpoints. This is an incomplete feature.
- **Confidence**: likely (would need to verify OpenAPI spec for each endpoint)

---

### 3. Documentation Issues

#### 3a. README variant count is wrong — missing `CrossRerank`
- **File**: `README.md:31`
- **Issue**: States "21 variants" but `PlannedOperation` actually has **22 variants** (including `CrossRerank`). The table (lines 37-56) also does not list `CrossRerank` at all.
- **Confidence**: confirmed

#### 3b. `routing::try_route` doc comment is misleading
- **File**: `src/routing.rs:88-91`
- **Issue**: Comment says "Fallible route construction (planner + REST projection)" but the function is just a 1-line delegation to `crate::plan::try_route`. It's a trivial re-export wrapper, which is itself duplicated — `plan::try_route` already exists and is re-exported in `lib.rs`.
- **Confidence**: confirmed

#### 3c. Stale comment about `shard_keys` in `create_collection_rest_body`
- **File**: `src/ddl.rs:381`
- **Issue**: Comment says "`shard_keys` is not part of CreateCollection — create keys via /shards after." However, the IR struct `CreateCollectionRequest` has `shard_keys: Option<Vec<String>>` (line 685) and it IS populated by `fill_collection_config` (line 180). The comment contradicts the code.
- **Confidence**: confirmed

#### 3d. Missing module-level documentation
- **Files**: `src/mutation.rs`, `src/ddl.rs`, `src/embedding.rs`
- **Issue**: These modules have no `//!` doc comments. Only `plan.rs` and `semantic.rs` have module-level docs. The `types.rs` file uses section comments (`// ── Filter types ──`) but no doc comment.
- **Confidence**: confirmed (minor)

---

### 4. Architecture Issues

#### 4a. `shard_key()` method on `PlannedOperation` only covers 6 of 22 variants
- **File**: `src/plan.rs:191-202`
- **Issue**: The `shard_key()` method handles only `Query`, `QueryGroups`, `GetPoints`, `Scroll`, `Count`, `Upsert`, `Delete` — but `UpdatePayload`, `ClearPayload`, `UpdateVectors`, `DeleteVectors`, `CreateShardKey`, `DropShardKey` also carry shard key information in their request types. The fallthrough `_ => None` silently drops shard key info for these operations.
- **Confidence**: confirmed

#### 4b. Circular routing: `routing::try_route` is a thin delegate, duplicating `plan::try_route`
- **File**: `src/routing.rs:89-91` delegates to `crate::plan::try_route`
- **File**: `src/plan.rs:912-914` defines `pub fn try_route`
- **Issue**: `lib.rs` re-exports `plan::try_route` directly on line 13. Meanwhile `routing::try_route` (line 89) is a separate public function that just delegates. Both are public. The `routing::route` function (line 76) swallows errors. The architecture has overlapping, confusing entry points.
- **Confidence**: confirmed

#### 4c. `alloc` vs `std` inconsistency
- **Files**: `src/types.rs:1-2` and `src/semantic.rs:7-8` use `alloc::string::String` + `alloc::vec::Vec`; but `src/routing.rs:98-99` uses `std::collections::HashMap`. The crate declares `extern crate alloc` in `lib.rs:1` (suggesting no_std support), but the `std::collections::HashMap` usage means it silently requires `std`.
- **Confidence**: confirmed

#### 4d. `QueryRequest.lookup_from` is always set to `None`
- **File**: `src/query.rs:399`
- **Issue**: In `lower_query_request`, `lookup_from: None` is hardcoded. Yet `QueryRequest` has the field (`types.rs:278-279`), and `PrefetchRequest` does properly set `lookup_from` (query.rs:348). This means top-level query `lookup_from` is never populated — if the Qdrant API supports it, this is an incomplete feature.
- **Confidence**: likely

#### 4e. `CountRequest.exact` is always set to `None`
- **File**: `src/plan.rs:346`
- **Issue**: The `exact` field of `CountRequest` is hardcoded to `None` in the planner despite the field existing in the type. The Qdrant `/points/count` endpoint accepts `exact` as a parameter.
- **Confidence**: confirmed

---

### 5. Removed / Changed Abstractions

- **No findings**: No code references `SELECT`, `QueryMode`, or other known removed abstractions.

---

### 6. TODO / FIXME / HACK Comments

- **No findings**: Zero instances of `TODO`, `FIXME`, `HACK`, or `XXX` in the entire crate. Either the code is clean (unlikely given other issues) or these markers were removed but their underlying issues remain.

---

### 7. Cargo.toml — Unused Dependencies

- **No unused dependencies found**: All three declared dependencies (`qql-core`, `serde`, `serde_json`) are actively used.
- **Redundant dev-dependencies**: `qql-core` and `serde_json` in `[dev-dependencies]` duplicate the `[dependencies]` section (lines 24-25).
- **No `[features]` beyond `default = []`**: The README mentions `serde` and `std` features (lines 135-137), but Cargo.toml has no such features defined. These are documentation-only.

---

### Summary of Issues by Severity

| Severity | Count | Key Items |
|----------|-------|-----------|
| High | 2 | `CrossRerank` route stub (no body), `embedding` module dead code (345 lines) |
| Medium | 6 | Dead `Method::as_str()`, dead `MinShould`, dead `embedding_has_wait`, dead `lower_point_vectors`, dead `MatchValue::TextAny`/`Except`, `shard_key()` incomplete |
| Low | 5 | README variant count off, stale `shard_keys` comment, `alloc`/`std` inconsistency, `query.lookup_from` always None, `count.exact` always None |
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive qql-runtime crate",
  "subagent_type": "rust-explore",
  "prompt": "Explore the qql-runtime crate at /data/codebases/qql-rs/crates/qql-runtime thoroughly. Find:\n1. Dead code / unused functions / unused types\n2. Half-baked features or incomplete implementations  \n3. Documentation issues (missing docs, stale comments)\n4. Mismatches with openapi.json contract - check contract_test.rs and compare with actual response shapes\n5. TODO/FIXME/HACK comments  \n6. gRPC implementation issues in grpc.rs and grpc_route.rs\n7. Error handling issues (panics, unwraps, expect without justification)\n8. Check Cargo.toml for unused dependencies\n9. Look at executor.rs, rest_adapter.rs, http_embedder.rs, prepare.rs for issues\n\nReport file names, line numbers, and specific issues found."
}
```

**Output:**
```
<task id="ses_0523a5ae6ffeLYWr7mzzvHXOuY" state="completed">
<task_result>
Now I have all the information needed. Here is my comprehensive report:

---

# qql-runtime Crate Audit Report

## 1. Dead Code / Unused Functions / Unused Types

### Functions explicitly `#[allow(dead_code)]`

| File | Line | Function | Notes |
|------|------|----------|-------|
| `grpc_route.rs` | 282 | `vector_params_diff` | Full implementation, never called from production code |
| `grpc_route.rs` | 293 | `vectors_config_diff` | Full implementation, never called from production code |
| `executor/dml/config.rs` | 6 | `Executor::resolve_dense_model` | Defines dense model resolution logic but unused |
| `executor/dml/config.rs` | 24 | `Executor::resolve_sparse_model` | Defines sparse model resolution logic but unused |
| `executor/dml/config.rs` | 56 | `Executor::cloud_model_options` | Returns cloud model options map but unused |

### Types possibly dead

| File | Line | Type | Notes |
|------|------|------|-------|
| `backend.rs` | 197 | `impl CollectionSchema {}` | Empty impl block — no methods defined, entirely useless |
| `client.rs` | 66–74 | `QdrantOpsBound` trait and blanket impls | Marker trait only used as supertrait bound; works but adds complexity |

### Generated code suppression

| File | Line | Suppression |
|------|------|-------------|
| `qdrant.rs` | 3 | `#![allow(dead_code)]` — entire generated type file |
| `qdrant_grpc.rs` | 3 | `#![allow(dead_code)]` — entire generated proto file |

---

## 2. Half-Baked Features / Incomplete Implementations

### gRPC stubs that return errors

| File | Line | Method | Error message |
|------|------|--------|---------------|
| `grpc.rs` | 451–456 | `GrpcQdrant::update_collection` | `"update_collection: use execute_route for gRPC"` |
| `grpc.rs` | 468–473 | `GrpcQdrant::create_field_index` | `"create_field_index: use execute_route for gRPC"` |
| `grpc.rs` | 476–485 | `GrpcQdrant::delete_field_index` | `"delete_field_index: use execute_route for gRPC"` |

These are direct implementations of `QdrantOps` trait methods. They are stubs that tell callers to use a different path. However:
- `PlannedOperation::CreateIndex` IS handled in `grpc_route.rs` (line 894) via `create_field_index_raw`
- `PlannedOperation::UpdateCollection` IS handled in `grpc_route.rs` (line 864) via `update_collection_raw`
- `PlannedOperation::DropIndex` IS handled in `grpc_route.rs` (line 926) via `delete_field_index_raw`

So the gRPC trait stub methods are bypassed for the planned-operation path, but the `QdrantOps::create_field_index` and `QdrantOps::delete_field_index` direct trait methods remain stub errors.

### Redundant conversion layers

**`rest.rs:200–223`**: `RestQdrant::create_collection` takes a `CreateCollectionReq`, immediately converts it into a `qql_plan::types::CreateCollectionRequest`, then delegates to `create_collection_planned`. This double-conversion (already typed → serialize → deserialize) is unnecessary overhead.

### Unused tests

| File | Line | Test | Status |
|------|------|------|--------|
| `tests/live_integration_test.rs` | 7 | `test_live_rest_and_grpc_with_ollama_embeddings` | `#[ignore = "requires local Qdrant ..."]` — never runs in CI |

---

## 3. Documentation Issues

### Missing docs on public items

| File | Line | Item |
|------|------|------|
| `executor/mod.rs` | 89 | `BatchKey` enum — no doc comment |
| `executor/mod.rs` | 95 | `fn statement_batch_key` — no doc comment |
| `executor/mod.rs` | 118 | `fn planned_batch_key` — no doc comment |
| `executor/mod.rs` | 149 | `Executor` struct — brief doc but missing many field docs |
| `executor/dml/config.rs` | 6 | `resolve_dense_model` — no doc comment |
| `executor/dml/config.rs` | 24 | `resolve_sparse_model` — no doc comment |
| `executor/dml/config.rs` | 41 | `inference_mode` — no doc comment |
| `executor/dml/config.rs` | 51 | `uses_local_embeddings` — no doc comment |
| `executor/dml/query.rs` | 24 | `topology_names_from_info` — no doc comment |
| `executor/dml/embedding.rs` | 10 | `resolve_embeddings` — brief doc but thin |
| `embedder.rs` | 84 | `HttpEmbedder` struct fields — most have doc, but `client` field has none |

### Files with blanket `#![allow(missing_docs)]`

| File | Lines | Impact |
|------|-------|--------|
| `qdrant.rs` | 2 | Entire generated OpenAPI types file |
| `qdrant_grpc.rs` | 2 | Entire generated gRPC types file |

### Stale comments

| File | Line | Comment |
|------|------|---------|
| `executor/mod.rs` | 214 | `// --- explain_stmt removed --- moved to qql_core::explain` — stale artifact |
| `embedder.rs` | 2–3 | `//! Embedding adapters for the runtime. The shared [`Embedder`] trait...` — says "This module re-exports them and provides [`HttpEmbedder`]" but `HttpEmbedder` is defined in this file, not re-exported |

---

## 4. Mismatches with openapi.json Contract

### Response shape fragility

**`executor/dml/query.rs:54–91`** — `extract_search_hits` tries three fallback paths:
1. `result.points` array (gRPC-style)
2. `points` array (REST-style)
3. `result` as array (batch result)

This multi-fallback parsing works but is fragile — it silently accepts any of these shapes without validating which is correct.

### Envelope validation divergence

**`rest.rs:427–453`** — `validate_success_envelope` requires **both** `result` field AND `status: "ok"`. However:
- Qdrant's mutation endpoints return `{ "result": { "status": "completed" }, "status": "ok", "time": ... }`
- Some gRPC responses use `time: 0.0_f64` (hardcoded in mutation_response, `grpc_route.rs:476`) which uses a different time value than the real server

### gRPC mutation response vs REST response

**`grpc_route.rs:473–479`** — `mutation_response()` always returns:
```json
{ "result": { "status": "completed" }, "status": "ok", "time": 0.0 }
```

But the REST backend returns real `time` from the server. This mismatch means gRPC results have fake `time: 0.0` while REST results have real timing — consumers can't rely on `time` consistently.

### CollectionInfo schema mismatch

**`grpc.rs:390–406`**: gRPC's `get_collection_info` returns `CollectionInfo` with `points_count` from `info.points_count.unwrap_or(0)` and `segments_count` from `info.segments_count` (proto `u64`), while REST backend (`rest.rs:179–198`) deserializes from JSON. The struct shapes align but the gRPC path uses protobuf field numbering while REST JSON uses field names — they could diverge if the schema evolves differently.

### OpenAPI used but not version-tracked

**`contract_test.rs:33–44`**: The contract tests compile jsonschema validators from `openapi.json` but there is no version constraint on which Qdrant version the schema corresponds to. If `openapi.json` is updated to a newer Qdrant version, the tests may fail against older servers or pass against incompatible schemas.

---

## 5. TODO/FIXME/HACK Comments

**None found.** The grep across all `.rs` files returned zero matches for `TODO`, `FIXME`, `HACK`, `XXX`, `todo!`, or `unimplemented`. This is unusual — either the codebase is meticulously clean, or issues are tracked externally.

---

## 6. gRPC Implementation Issues

### Stub implementations (see section 2)

**`grpc.rs:451–456, 468–473, 476–485`**: Three `QdrantOps` trait methods are error-returning stubs.

### Missing gRPC field coverage

**`grpc_route.rs:264–280`**: `vector_params()` sets `datatype: None` and ignores the `datatype` field from OpenAPI (always uses default):
```rust
datatype: None,
```
The OpenAPI schema supports a `datatype` per-vector field (`float32`, `uint8`, `float16`), but the gRPC converter never reads it.

### gRPC `create_collection` doesn't propagate `datatype`

**`grpc_route.rs:759–819`**: The `CreateCollection` gRPC builder (`grpc_route.rs:761`) maps vectors but the `vector_params` function (line 264) hardcodes `datatype: None`. Users specifying `WITH DATATYPE 'uint8'` in QQL will silently lose that setting over gRPC.

### Unused but implemented gRPC DDL converters

**`grpc_route.rs:282, 293`**: `vector_params_diff` and `vectors_config_diff` are fully implemented but marked `#[allow(dead_code)]`. These handle `PATCH /collections/{name}` diffs but the `UpdateCollection` planned operation builds the diff inline in `execute_planned_grpc` (line 864) instead.

### `read_fan_out_delay_ms` field type

**`grpc_route.rs:348`**: `read_fan_out_delay_ms` — the Qdrant proto expects `u64` but the actual proto field might be `Duration`. No validation that this maps correctly.

---

## 7. Error Handling Issues

### Production-code `unwrap()` / `expect()` without justification

| File | Line | Expression | Risk |
|------|------|------------|------|
| `executor/mod.rs` | 492 | `pending.pop().expect("pending contains one operation")` | Panics if batch logic has a bug (single-item path) |
| `executor/mod.rs` | 523 | `serde_json::to_value(hits).unwrap_or_default()` | Silently swallows serialization errors |
| `executor/mod.rs` | 877 | `serde_json::to_value(hits).unwrap_or_default()` | Same pattern in `dispatch_planned` |
| `executor/mod.rs` | 1038 | `serde_json::to_value(out).unwrap_or_default()` | Same pattern in cross-rerank |
| `executor/mod.rs` | 916 | `unreachable!("handled above")` | Panics if a new PlannedOperation variant is added but not handled |
| `rest.rs` | 24 | `Self::with_timeout(...).expect("failed to build reqwest client")` | Constructor panics on config errors — contradicts comment on `with_timeout` saying "Fallible so library constructors never panic" |
| `executor/dml/upsert.rs` | 221 | `model.unwrap()` | Panics if `model` is `None` |
| `executor/dml/config.rs` | 15 | `cfg.embedding_model.as_ref().unwrap().clone()` | Unwrap after checking non-empty, but clone still happens |
| `executor/dml/config.rs` | 18 | `cfg.inference_model.as_ref().unwrap().clone()` | Same pattern |

### Build script panics

| File | Line | Expression | Notes |
|------|------|------------|-------|
| `build.rs` | 39 | `.expect("Failed to read openapi.json")` | Build scripts panicking is common but these could provide more context |
| `build.rs` | 41 | `.expect("Invalid OpenAPI JSON")` | |
| `build.rs` | 109 | `.unwrap_or_else(...)` / `panic!` | |
| `build.rs` | 118 | `.expect("Failed to parse generated Rust tokens")` | |
| `build.rs` | 122 | `.unwrap()` for OUT_DIR | |
| `build.rs` | 133 | `.expect("failed to locate the vendored protoc binary")` | Blocks grpc builds if protoc is broken |

### Silent error swallowing

**`embedder.rs:265`**: `resp.text().await.unwrap_or_default()` — if reading the error body fails, the error message is just empty string.

**`grpc_route.rs:1654`**: `n.as_u64().unwrap_or(0)` — silently truncates negative JSON numbers to zero.

### Test code panics

The test files (`grpc_route.rs` tests, `contract_test.rs`, `executor_test.rs`) contain dozens of `.unwrap()`, `.expect()`, and `panic!()` calls. This is acceptable for test code but makes tests brittle — adding a new variant can cause widespread panics.

---

## 8. Unused Dependencies in Cargo.toml

| Dependency | Status | Evidence |
|------------|--------|----------|
| `chrono` = "0.4" | **Unused** | Not `use`d anywhere in any `.rs` file under `src/` or `tests/` |
| `uuid` = "1" | **Unused** | Not `use`d anywhere in any `.rs` file. Only appears in test string literals ("uuid-v4") and backend PointId types (which don't use the crate) |
| `futures-util` = "0.3" | **Unused** | Not `use`d anywhere in any `.rs` file |
| `prost` = "0.14" | **Likely unused** | Not directly imported. Tonic uses prost internally, but the explicit dep may not be needed |
| `prost-types` = "0.14" | **Likely unused** | Not directly imported. Same as prost — may be transitive from tonic |

The following build dependencies are only used in `build.rs` and are correctly specified:
- `typify`, `schemars`, `prettyplease`, `syn` — all used in `build.rs`
- `tonic-prost-build`, `protoc-bin-vendored` — used in `build.rs` under `#[cfg(feature = "grpc")]`

---

## 9. File-by-File Specific Issues

### `executor/mod.rs`

| Line | Issue |
|------|-------|
| 2 | `use std::collections::HashMap` — used? checked in `execute_cross_rerank` at line 938, but imported at module level |
| 16 | `pub use qql_embed::resolve::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME}` — re-exports, fine |
| 147 | `pub use crate::client::*` — glob re-export, may pull in unintended items |
| 201–212 | `explain`, `explain_all`, `explain_node` — thin wrappers over `qql_core::explain` |
| 637–741 | `prepare_statement` is a ~100-line method doing too many things: embedding resolution, vector validation, collection auto-creation |
| 928–1040 | `execute_cross_rerank` is a ~110-line method in a single function — very long |

### `rest.rs`

| Line | Issue |
|------|-------|
| 1 | `use std::time::Duration` — fine |
| 23–24 | `Self::with_timeout(...).expect(...)` — `RestQdrant::new` panics on invalid config |
| 61–125 | `call_body` — uses `url_buf` cloning on every error path, could use `.with_url()` lazily |
| 200–223 | `create_collection` redundant conversion (see section 2) |
| 354 | `execute_http` — `format!("{}{}", self.base_url, route.path)` can double-slash if `base_url` ends with `/` (even though `with_client` trims it, `with_timeout` doesn't) |
| 427–453 | `validate_success_envelope` — requires `status: "ok"` but Qdrant mutations return `status: "completed"` for individual results (though wrapped in `status: "ok"`) |

### `embedder.rs` (HttpEmbedder)

| Line | Issue |
|------|-------|
| 243–282 | `do_request` — error response text uses `unwrap_or_default()` (loses detail) |
| 284–290 | `resolve_dense_model` — checks `model != "default"` but `"default"` is a hardcoded sentinel not documented in QQL |
| 313–403 | `embed_batch_with_model` — validates dimension but the error message repeats "index {}" — could be confusing in production |
| 409–515 | `embed_multi_batch_with_model` — long method with very similar structure to `embed_batch_with_model` (significant code duplication) |

### `backend.rs`

| Line | Issue |
|------|-------|
| 197 | `impl CollectionSchema {}` — empty impl block, no purpose |
| 216–324 | `schema_from_rest_result` — 100+ line function with deep nesting and no unit tests |

### `grpc.rs`

| Line | Issue |
|------|-------|
| 451–456, 468–473, 476–485 | Three stub implementations returning errors (see section 2) |
| 630–658 | `vector_params_to_spec` — uses `try_from(p.distance)` then `as_str_name()` — could produce unexpected strings for unknown enum values |

### `grpc_route.rs`

| Line | Issue |
|------|-------|
| 282 | `vector_params_diff` — dead code |
| 293 | `vectors_config_diff` — dead code |
| 348 | `read_fan_out_delay_ms` — potential field-type mismatch with proto |
| 264 | `datatype: None` — missing datatype support |
| 473–479 | `mutation_response()` — hardcoded `time: 0.0_f64`, not real server time |
| 1650–1664 | `to_point_id_json` — `unwrap_or(0)` silently truncates negative numbers |
| 2582–2590 | `quantization_config_to_json` — `Turboquant` variant maps to empty `{}` — missing turbo-specific fields |

### `executor/dml/upsert.rs`

| Line | Issue |
|------|-------|
| 221 | `model.unwrap()` — potential panic if `model` is `None` |
| 133–170 | `ensure_collection_for_upsert` — creates collection with minimal schema, no error if the collection exists with incompatible schema |
| 172–236 | `resolve_dense_vector_size` — deeply nested `if-else` with 6 levels, hard to follow |

### `executor/dml/config.rs`

| Line | Issue |
|------|-------|
| 6, 24, 56 | Three dead-code methods (see section 1) |
| 41–49 | `inference_mode()` — trims but doesn't validate against known modes |

### `executor/dml/query.rs`

| Line | Issue |
|------|-------|
| 24–52 | `topology_names_from_info` — takes both `vectors` and `dense_vectors` fields; the fallback logic is implicit |
| 54–91 | `extract_search_hits` — three fallback paths for parsing responses; fragile |

### `executor/dml/embedding.rs`

| Line | Issue |
|------|-------|
| 10–16 | Thin delegation to `qql_embed` — existence of this file seems unnecessary; could inline in `executor/mod.rs` |

### `contract_test.rs`

| Line | Issue |
|------|-------|
| 19–31 | `load_openapi_json` returns `Option` — silently skips tests if `openapi.json` is missing |
| 33–44 | `validate_ref` uses `unwrap_or_else(|e| panic!(...))` — test code, acceptable |
| 213–224 | "IMAGE is embed-time" test — only tests post-embed shape, not the IMAGE document handling path |
| 579–612 | `ddl_create_grpc_reads_same_ir_as_rest_projection` — uses `let _ = hnsw;` line 610 (dead assignment) |

### `build.rs`

| Line | Issue |
|------|-------|
| 127 | `#[cfg(feature = "grpc")]` block — protobuf compilation only happens when grpc feature is enabled, but `rerun-if-changed=proto/` runs unconditionally |
| 39, 41, 109, 118, 122, 133 | Multiple `.expect()` calls that panic on build failures — acceptable for build scripts but could be more informative |

### `qdrant.rs` / `qdrant_grpc.rs`

| Line | Issue |
|------|-------|
| qdrant.rs:1–6 | Generated code suppression — but qdrant.rs seems unused entirely (`rest.rs` doesn't use it, only `grpc.rs` uses `qdrant_grpc.rs`) |
| qdrant_grpc.rs:1–6 | Generated code suppression — acceptable for generated code |

---

## Summary of Most Impactful Issues

1. **3 gRPC trait methods are unimplemented stubs** (`grpc.rs:451,468,476`) — but their DDL analogs work via the planned-operation bypass
2. **5 explicitly dead-code-marked functions** (config.rs:6,24,56; grpc_route.rs:282,293)
3. **3 declared but unused runtime dependencies** (`chrono`, `uuid`, `futures-util`)
4. **`datatype` field silently dropped** in gRPC vector params (`grpc_route.rs:264`)
5. **`RestQdrant::new()` panics** on invalid config (`rest.rs:24`)
6. **gRPC mutation responses return fake `time: 0.0`** (`grpc_route.rs:473–479`)
7. **`prepare_statement` is a 100-line god method** (`executor/mod.rs:637–741`)
8. **`extract_search_hits` uses fragile multi-path response parsing** (`executor/dml/query.rs:54–91`)
9. **No TODO/FIXME/HACK comments** — likely tracked externally
10. **OpenAPI contract tests only validate request bodies**, never gRPC response shapes
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive qql-embed crate",
  "subagent_type": "rust-explore",
  "prompt": "Explore the qql-embed crate at /data/codebases/qql-rs/crates/qql-embed thoroughly. Find:\n1. Dead code / unused types / unused functions\n2. Half-baked features or incomplete implementations\n3. Documentation issues (missing docs, stale comments)\n4. Check Embedder trait and all implementations for issues\n5. Compare with fastembed-rs.md at /data/codebases/qql-rs/fastembed-rs.md - are all the supported embedding types (text, sparse, image, rerank, BGE-M3 joint, Qwen3) properly supported? Any models missing?\n6. TODO/FIXME/HACK comments\n7. Check Cargo.toml for unused dependencies\n8. Look at sparse.rs, bm25.rs, resolve.rs\n\nReport file names, line numbers, and specific issues found."
}
```

**Output:**
```
<task id="ses_0523a500cffeORr1tjOzZvhYnu" state="completed">
<task_result>
Now I have a thorough understanding of the entire crate. Here is my comprehensive report.

---

## Comprehensive qql-embed Crate Audit Report

---

### 1. DEAD CODE / UNUSED TYPES / UNUSED FUNCTIONS

#### 1a. `SparseEmbedder` — completely dead struct

**File:** `embedder.rs:175-181`  
**Issue:** `SparseEmbedder` is defined as a public struct with a single method `embed_sparse(text) -> SparseVector` that delegates to `sparse::build_query_default(text)`. It is re-exported through `lib.rs:22` and `qql-runtime/src/embedder.rs:17`, but **no code in the entire repository ever constructs `SparseEmbedder` or calls its method**. Both `qql-runtime` and `qql-edge` call `qql_embed::sparse::build_query_default(text)` directly (via paths like `qql-runtime/src/embedder.rs:670` and `qql-edge/src/embedder.rs:798`).

**Confidence:** Confirmed — zero instantiation sites across the whole codebase.

#### 1b. `IdentityHasher` — unnecessarily public

**File:** `sparse.rs:16`  
**Issue:** `IdentityHasher` is declared `pub` but only used internally via the private `FastMap` type alias (`sparse.rs:37`). It is leaked into the public API through `qql-runtime/src/sparse.rs` which does `pub use qql_embed::sparse::*;`. No external crate imports `IdentityHasher` directly (confirmed by grep). Should be private.

**Confidence:** Confirmed — no external references.

#### 1c. `hash_token` — unnecessarily public

**File:** `sparse.rs:39`  
**Issue:** Public function used only within `sparse.rs` (in `build_query`/`build_document` for the Unicode tokenization path) and in `sparse_test.rs`. Leaked via the `*` re-export. No external consumers.

**Confidence:** Confirmed — grep shows no usage outside tests and internal sparse.rs code.

#### 1d. `build_document` — unnecessarily public

**File:** `sparse.rs:349`  
**Issue:** `pub fn build_document(text, k1, b, avgdl)` is the raw BM25 document builder. The only external API-level consumer is `build_document_default` (line 404). `build_document` itself is not called directly by any external code — the benchmark (`bench_sparse.rs`) uses `build_document_default`. Leaked via the `*` re-export.

**Confidence:** Confirmed — no external direct callers.

#### 1e. `tokenize` — unnecessarily public

**File:** `sparse.rs:76`  
**Issue:** Public but only used internally (by `build_query`/`build_document` for Unicode text) and in `sparse_test.rs`. Leaked via `*` re-export. No external consumers.

**Confidence:** Confirmed — no external callers.

#### 1f. `utils` mentioned in doc but doesn't exist

**File:** `sparse.rs`  
**Issue:** No `bm25.rs` file exists anywhere in the repository. The glob search for `**/bm25*` returned zero results. The BM25 implementation lives entirely in `sparse.rs`.

**Confidence:** Confirmed — file does not exist.

---

### 2. HALF-BAKED FEATURES / INCOMPLETE IMPLEMENTATIONS

#### 2a. Sparse model selection is always rejected at the resolve layer

**Files:** `resolve.rs:137-143`, `resolve.rs:776-781`  
**Issue:** The `EmbeddingSpec::Sparse { model: Some(...) }` variant is parsed by the AST but immediately rejected at embedding resolution time:

```rust
if model.is_some() {
    return Err(QqlError::execution(
        "QQL-EMBEDDING",
        "sparse model selection is not supported by the local BM25 sparse embedder; omit MODEL on EMBED SPARSE",
        None,
    ));
}
```

The **root cause** is that the `Embedder` trait's `embed_sparse` method signature (`async fn embed_sparse(&self, text: &str)`) takes no `model` parameter, so no implementation could ever support model-based sparse embedding through this interface. If a fastembed-based embedder wanted to support Splade_PP_en_v1 or BGE-M3 sparse models, it couldn't express that through the trait.

**Confidence:** Confirmed — this is a design limitation that prevents model-based sparse embedding entirely.

#### 2b. Rerank, image, and multi-vector embedding all default to "unsupported"

**File:** `embedder.rs:62-115`  
**Issue:** The default implementations of `embed_multi`, `embed_image`, and `rerank_pairs` all return errors via `multi_unsupported_error`, `image_unsupported_error`, and `cross_rerank_unsupported_error` respectively. This means every embedder host must explicitly override these methods to support these features — there is no gradual fallback path.

**Confidence:** Confirmed — by design, but these are stub implementations.

#### 2c. BGE-M3 "joint" embedding not supported as a single operation

**File:** `resolve.rs:801-849`  
**Issue:** The `EmbeddingSpec::Hybrid` variant calls `embed_dense_batch` and `embed_sparse` as separate operations. There is no way to do a single forward pass that produces dense+sparse+ColBERT simultaneously as the BGE-M3 model supports. The fastembed-rs.md document (lines 317-348) explicitly describes BGE-M3 joint embedding as producing all three outputs from one call, but qql-embed has no concept of a joint embed operation.

**Confidence:** Confirmed — the `Embedder` trait lacks a `embed_joint` or similar method.

#### 2d. Auto-embedding on UPSERT without schema produces both dense AND sparse unconditionally

**File:** `resolve.rs:67-101`  
**Issue:** When there's no `embedding` or `embed` directive on an UPSERT, the auto-embedding path always creates **both** a `"dense"` and `"sparse"` vector per point (lines 90 and 92). It doesn't check the collection topology to see if one or both vector types actually exist. If only dense is configured, every point gets an unwanted sparse vector too.

**Confidence:** Confirmed — always adds both vectors regardless of topology.

#### 2e. `unknown_using_kind_error` is public but unused within the crate

**File:** `topology.rs:456-464`  
**Issue:** This function is `pub` and re-exported via `lib.rs`, but within qql-embed it's only called once at `resolve.rs:715` via `crate::topology::unknown_using_kind_error`. This is fine — not an issue per se, but worth noting it's the only consuming site.

---

### 3. DOCUMENTATION ISSUES

#### 3a. `SparseEmbedder` has no doc comment on its `embed_sparse` method

**File:** `embedder.rs:175-180`  
**Issue:** The struct gets a doc comment ("Local sparse-only helper (no dense model).") but the `impl` block method `embed_sparse` has no doc comment at all, unlike every other public function in the trait.

#### 3b. `resolve_query_vector_kinds` doc references offline schema resolution but not async flow

**File:** `topology.rs:34-37`  
**Issue:** The doc says "Fill `USING` kinds (and omitted targets) from collection topology." but doesn't explain that this must be called **before** `resolve_embeddings` in the execution pipeline. The developer must infer the ordering from the module-level doc.

#### 3c. No doc comments on `EmbedderBound` trait methods (both cfg variants)

**File:** `embedder.rs:7-14`  
**Issue:** `EmbedderBound` is a marker trait with two cfg-gated definitions, but neither has any documentation explaining WHY it exists (i.e., WASM `?Send` compat). This is not obvious to a new reader.

#### 3d. The `EmbeddingSpec::Multi` variant doc mentions DENSE+SPARSE+MULTI but not IMAGE

**File:** `qql-core/src/ast/statement.rs:400-401`  
**Issue:** The comment says "Combined specs (e.g. DENSE + SPARSE + MULTI VECTOR colbert)." but doesn't mention that IMAGE can also be combined. This is a stale/outdated doc comment in the dependency, not in qql-embed itself, but relevant.

#### 3e. No TODO/FIXME/HACK comments found

A grep for `TODO|FIXME|HACK|XXX` across all `.rs` files in qql-embed returned **zero results**. This either means the code is very clean or that known issues aren't being tracked inline.

---

### 4. EMBEDDER TRAIT AND IMPLEMENTATION ISSUES

#### 4a. `embed_sparse` lacks a `model` parameter — design asymmetry

**File:** `embedder.rs:25`  
**Issue:** All other embedding methods (`embed_dense`, `embed_multi`, `embed_image`) accept a `model: &str` parameter to select which model to use. `embed_sparse` does not. This means:
- The AST supports `EMBED SPARSE MODEL 'splade'` but it's always rejected
- No runtime embedder can offer model-based sparse embedding
- The `EmbeddingSpec::Sparse { model: Some(...) }` variant is dead weight in the AST for this crate's purposes

#### 4b. `dimension()` and `multi_dimension()` are never called within qql-embed

**File:** `embedder.rs:29-36`  
**Issue:** These methods return `Option<usize>` but no code in the qql-embed crate calls them. They are only overridden in the runtime (`qql-runtime/src/embedder.rs:642-650`) and edge (`qql-edge/src/embedder.rs:704-708`) implementations. The information is available but unused at this layer.

#### 4c. `accepts_model()` default returns `true` — no validation at the trait level

**File:** `embedder.rs:40-42`  
**Issue:** The default `accepts_model` returns `true` for every model. The actual validation of whether a model is supported is left entirely to downstream embedders. This means if a user passes a non-existent model name, it won't be caught until the embedder returns an error, which could be cryptic.

#### 4d. `SparseEmbedder` doesn't implement `EmbedderBound` or `Embedder`

**File:** `embedder.rs:175-181`  
**Issue:** The `SparseEmbedder` struct is presented as a "Local sparse-only helper" but doesn't implement `EmbedderBound`, so it can't be passed as `&dyn Embedder` or used with any generic code expecting an embedder. It's essentially a namespace for a free function, not a useful type.

---

### 5. COMPARISON WITH fastembed-rs.md — SUPPORTED EMBEDDING TYPES

The fastembed-rs.md document describes these embedding categories:

| Category | In fastembed-rs.md? | In qql-embed? | Notes |
|---|---|---|---|
| **Text embedding (dense)** | ✅ Yes, ~30 models | ✅ Yes, via `embed_dense_batch` | Model selection supported |
| **Sparse embedding** | ✅ Yes, Splade + BGE-M3 sparse | ⚠️ **Partial** — local BM25 only, no Splade/BGE-M3 sparse | `embed_sparse` has no model param; model selection rejected |
| **Image embedding** | ✅ Yes, CLIP, ResNet, Unicom, etc. | ⚠️ **Trait-level** — default returns unsupported error; hosts must opt in | Correctly abstracted as optional |
| **Reranking** | ✅ Yes, 4 models | ⚠️ **Trait-level** — default returns unsupported error | Same pattern as image |
| **BGE-M3 Joint** (single pass, dense+sparse+ColBERT) | ✅ Yes, `Bgem3Embedding` | ❌ **Not supported** — no joint embed method in `Embedder` trait | `Hybrid` spec calls separate dense and sparse paths |
| **Qwen3** (text + VL multimodal) | ✅ Yes, `Qwen3TextEmbedding` + `Qwen3VLEmbedding` | ❌ **Not supported** — no Qwen3-specific types or methods | Would require new methods or a new trait |
| **Nomic v2 MoE** | ✅ Yes, `NomicV2MoeTextEmbedding` | ❌ **Not supported** | Candle-specific, not in the ONNX-based Embedder trait |
| **CLIP text encoder** (as text model) | ✅ Listed under text embeddings | ✅ Generic dense embedding path would handle this | Only if configured by the host |

**Summary:** qql-embed supports the **conceptual categories** (dense, sparse, multi, image, rerank) but does NOT support:
- BGE-M3 joint embedding (single-pass dense+sparse+ColBERT)
- Qwen3-specific embedding types
- Nomic v2 MoE
- Any model-based sparse embedding (Splade, BGE-M3 sparse)
- The 30+ specific model variants listed (appropriately — those belong in downstream embedders)

---

### 6. TODO/FIXME/HACK COMMENTS

**None found.** Zero matches across all `.rs` files in the crate. The codebase is clean of inline issue markers.

---

### 7. CARGO.TOML — UNUSED DEPENDENCIES

**File:** `Cargo.toml:16-21`

```
[dependencies]
qql-core = { ... }          # ✅ Used extensively in all modules
async-trait = "0.1"          # ✅ Used in embedder.rs (#[async_trait])

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }  # ✅ Used in tests (#[tokio::test])
```

**No unused dependencies found.**

---

### 8. SPECIFIC FILE ANALYSIS

#### `sparse.rs` — BM25 sparse vector implementation

**Issues:**
- **Duplicate logic:** The hyphen-splitting logic (`if !slice.contains(&b'-')` ... else branch) appears **four times** identically — once in `append_tokens` (line 139-167) and three times in `hash_tokens_ascii_fast` (lines 240-298). This is a ~30-line code block duplicated verbatim.
- **`IdentityHasher` is public** but should be private (see §1b).
- **`hash_token_bytes` vs `hash_token`:** Two separate hash functions exist (`hash_token_bytes` has inline lowercase conversion, `hash_token` uses `to_lower_ascii`). The fast path uses `hash_token_bytes`; the Unicode path uses `hash_token`. This is intentional but worth noting as a maintenance risk.
- **`is_valid_token_len` has a hardcoded special case for 'c'** (`line 220: first_byte == b'c'`), which is replicated in `maybe_token` (`line 131: b == b'c'`). The rationale for preserving single-character 'c' as a valid token is undocumented.

#### `bm25.rs` — File does not exist

The user specifically asked about this file but it doesn't exist anywhere in the repository. The BM25 implementation is entirely within `sparse.rs`.

#### `resolve.rs` — Embedding resolution

**Issues:**
- **Dead parameter `collection`** on `configure_query` and `configure_expr` in `topology.rs` — the `collection: &str` parameter is threaded through `topology.rs:185-285` but only actually used in error message formatting (`unknown_vector_error`, etc.). This is a minor API smell.
- **Auto-embedding always adds sparse vector** without checking topology (see §2d).
- **`ensure_batch_len`** (`resolve.rs:720`) is a standalone function that's only called in a few places; elsewhere the same check is done inline with custom error messages (e.g., `resolve.rs:376`, `resolve.rs:864`).

#### `topology.rs` — Vector kind resolution

**Issues:**
- **`collection` parameter unused in execution** — threaded through all `configure_*` functions but only used for error messages. This is a necessity for good error reporting but results in many functions taking a parameter they don't functionally use.
- **`resolve_query_vector_kinds_simple`** (`line 48`) constructs a `TopologyNames` with empty `multivector` and delegates. It's a convenience wrapper but another function that could be simplified.

---

### SUMMARY OF MOST IMPORTANT FINDINGS

| # | Severity | Issue | File:Line |
|---|---|---|---|
| 1 | **High** | `embed_sparse` trait method lacks `model` param, making model-based sparse embedding impossible | `embedder.rs:25` |
| 2 | **High** | No support for BGE-M3 joint embedding (single-pass dense+sparse+ColBERT) | Trait design gap |
| 3 | **High** | Auto-embedding on UPSERT unconditionally adds both dense and sparse vectors regardless of topology | `resolve.rs:67-101` |
| 4 | **Medium** | `SparseEmbedder` is completely dead code — never instantiated anywhere | `embedder.rs:175-181` |
| 5 | **Medium** | `IdentityHasher`, `hash_token`, `build_document`, `tokenize` are unnecessarily public | `sparse.rs:16,39,76,349` |
| 6 | **Medium** | Hyphen-splitting logic duplicated 4× in `sparse.rs` (~120 lines of duplication) | `sparse.rs:139-298` |
| 7 | **Low** | No TODO/FIXME/HACK markers anywhere — known issues not tracked inline | Entire crate |
| 8 | **Low** | Missing doc on `SparseEmbedder::embed_sparse` | `embedder.rs:178` |
| 9 | **Low** | Single-character 'c' as a valid token is undocumented magic | `sparse.rs:131,220` |
| 10 | **Low** | `dimension()` and `multi_dimension()` are defined but never called within this crate | `embedder.rs:29-36` |
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive qql-edge crate",
  "subagent_type": "rust-explore",
  "prompt": "Explore the qql-edge crate at /data/codebases/qql-rs/crates/qql-edge thoroughly. Find:\n1. Dead code / unused types / unused functions\n2. Half-baked features - are all edge capabilities properly match the gaps.md edge capability matrix?\n3. Documentation issues\n4. Fastembed-rs integration issues - check against /data/codebases/qql-rs/fastembed-rs.md\n5. TODO/FIXME/HACK comments\n6. The unsupported.rs catalog - is it complete?\n7. Check Cargo.toml for unused dependencies\n8. Architecture issues\n\nReport file names, line numbers, and specific issues found."
}
```

**Output:**
```
<task id="ses_0523a4694ffex5ojPMSXNqAoov" state="completed">
<task_result>
Now I have a thorough picture. Here is my complete report.

---

## Full Report: qql-edge Crate Analysis

### 1. Dead Code / Unused Types / Unused Functions

**No truly dead code** was found — all public API functions are reachable, and all private items are used. However:

| Item | Location | Status | Notes |
|------|----------|--------|-------|
| `FastEmbedder::try_new(InitOptionsWithLength<EmbeddingModel>)` | `src/embedder.rs:118` | Public, never called internally | Exposed API but only `try_with_options` is used internally. Could be removed or marked `#[doc(hidden)]`. |
| `FastEmbedder::try_default()` | `src/embedder.rs:316` | Public, never called internally | Same pattern — public API surface not exercised internally. |
| `FastEmbedder::try_from_model(&str)` | `src/embedder.rs:321` | Public, never called internally | Same. |
| `resolve_reranker_model()` | `src/embedder.rs:635` | Public but **not re-exported** from `lib.rs` | All other `resolve_*` functions are re-exported. This one is only accessible as `qql_edge::embedder::resolve_reranker_model`. Inconsistency. |
| Missing `list_reranker_models()` | — | **Absent** | `list_embedding_models()` (line 488) lists text, BGE-M3, and image models but skips reranker models. Undocumented design choice but asymmetric with image/multi listing. |

**Confidence: confirmed**

---

### 2. Half-Baked Features vs gaps.md Edge Capability Matrix

**The edge capability matrix in gaps.md is accurately reflected in code.** Every gap status maps correctly:

| gaps.md Capability | Status | Code Match | Notes |
|---|---|---|---|
| Dense ANN + sparse + hybrid FUSION | **Yes** | ✓ `query_converter.rs:189-205` — Fusion::Rrf, Fusion::Dbsf | |
| Multivector store + query | **Yes** | ✓ `embedder.rs:172-216` — `multi_model` option, `Bgem3Embedding` | |
| `CROSS RERANK` / CLIP IMAGE | **Opt-in** | ✓ `embedder.rs:262-300` — `reranker_model`, `image_model` | |
| `GROUP BY` | **No** | ✓ `unsupported.rs:17` — `EdgeUnsupported::GroupBy` | |
| `SHARD` / ACORN / ALTER | **No** | ✓ `unsupported.rs:19-28` — All properly rejected | |
| `timeout` / `consistency` | **N/A** | ✓ Silently ignored (single-node) | |
| Batch RPC | **No** | ✓ `mod.rs:906-949` — fan-out to individual routes | |

**Individual gap tracking:**

| Gap ID | Status | Code Location | Notes |
|--------|--------|--------------|-------|
| GAP-MV/KIND | **DONE** | `embedder.rs:89-94` — multi/image/rerank slots | |
| GAP-EDGE-001/UX-008 | **DONE** | (CLI doctor, outside this crate) | |
| GAP-EDGE-002 (IMAGE = local path) | **PARTIAL** | `embedder.rs:878-909` — passes file path directly to fastembed | **No URL fetch implemented.** The `Embedder` trait doc says `source` is "filesystem path or URL" but FastEmbedder only handles paths. |
| GAP-EDGE-003 (heavy model e2e optional) | **PARTIAL** | `embedder.rs:172-300` — all multi/image/rerank models are optional | |
| GAP-EDGE-004 (Edge GROUP BY) | **OPEN** | `unsupported.rs:17, 39-40` | Acknowledged as open. |
| GAP-QUAL-002 (batch fan-out) | **OPEN** | `mod.rs:906-949` — `execute_query_batch` and `execute_update_batch` iterate individual routes | |
| GAP-UX-002 (MMR dense-only) | **PARTIAL** | `query_converter.rs:86-98` — MMR code accepts any `VectorInternal` | **No explicit guard** preventing MMR with sparse vectors. qdrant-edge may reject this at runtime but the conversion layer doesn't check. |

**Finding: gaps.md is an accurate representation of the edge crate's current state.** No discrepancies found.

**Confidence: confirmed**

---

### 3. Documentation Issues

| Issue | Location | Details |
|-------|----------|---------|
| `list_embedding_models` doc out of date | `embedder.rs:487-488` | Doc says "List every dense text model fastembed can load, plus offline multi (BGE-M3)". But the function also lists **image models** (lines 510-519). Missing "and image/CLIP vision models" in the doc. |
| `resolve_reranker_model` not re-exported | `lib.rs:38-41` vs `embedder.rs:635` | Three other `resolve_*` functions are re-exported in the `pub use embedder::{...}` block. `resolve_reranker_model` is excluded with no comment explaining why. |
| `FastEmbedder::try_new` takes raw `InitOptionsWithLength` | `embedder.rs:118` | No rustdoc on this method. Callers see `InitOptionsWithLength<EmbeddingModel>` instead of the more familiar `TextInitOptions`. Could confuse. |
| `FastEmbedder::try_default` and `try_from_model` | `embedder.rs:316, 321` | Have doc comments but don't explain trade-offs vs `try_with_options`. |
| No `image_dimension` on `Embedder` trait | `embedder.rs:716-717` (comment) | The code comment acknowledges this: "image_dimension is not on Embedder trait; use dimension() for dense CLIP text. Image dim available via FastEmbedder::image_dimension()." This is a **trait gap** — generic embedder users can't query image dimension. |
| README slightly outdated | `README.md:99` | Says "When neither feature is enabled, only `custom_executor()` is available" — accurate but doesn't mention `custom_executor_with_dimension`. |

**Confidence: confirmed**

---

### 4. Fastembed-rs Integration Issues

Checked against `/data/codebases/qql-rs/fastembed-rs.md`.

| Issue | Location | Details |
|-------|----------|---------|
| `InitOptionsWithLength` used directly | `embedder.rs:9, 118, 135` | fastembed-rs.md documents `TextInitOptions` (which is `InitOptionsWithLength<EmbeddingModel>`). Using the generic type name instead of the alias is valid but deviates from docs. |
| No `SparseTextEmbedding` integration | `embedder.rs` (entire file) | fastembed-rs.md lists SpladePPv1 and BGE-M3 sparse models. Edge uses `qql_embed::sparse::build_query_default()` (BM25-style) instead. **Design choice**, not a bug, but means learned sparse models are unavailable offline. |
| No Qwen3 / candle backend support | — | fastembed-rs.md lists `qwen3` feature and Qwen3-Embedding models. Not available in edge — users must use HTTP or custom embedder. |
| Image model list incomplete | `embedder.rs:510-519` | fastembed-rs.md lists 5 image models (CLIP, ResNet, Unicom x2, Nomic). The `resolve_image_model` function relies on `ImageEmbedding::list_supported_models()` which returns whatever fastembed exposes — should be complete at runtime, but `list_embedding_models` doesn't cross-reference against the doc. |
| `ort-download-binaries-rustls-tls` feature | `Cargo.toml:31` | Enables downloading ONNX Runtime with rustls. Uses network on first model load anyway (HuggingFace download). This is documented behavior. |
| `hf-hub-rustls-tls` feature | `Cargo.toml:32` | Required for HuggingFace model downloads. Correct. |
| `image-models` feature | `Cargo.toml:33` | Enables CLIP/ResNet vision models. Correct, since image embedding requires the `image` crate dependency. |

**Overall**: fastembed integration is sound. No API misuse, no version conflicts. The crate uses fastembed v5.17.3 (from Cargo.lock).

**Confidence: confirmed**

---

### 5. TODO/FIXME/HACK Comments

**None found.** The entire `src/` directory has zero TODO, FIXME, HACK, or XXX comments. The grep across the full `crates/qql-edge/` tree returned empty.

This is notable — the codebase is unusually clean of markers. Either all known issues are tracked elsewhere (gaps.md, issues), or development is in a polished state.

**Confidence: confirmed**

---

### 6. unsupported.rs Catalog Completeness

**The catalog is complete and well-structured.** Findings:

| Aspect | Verdict |
|--------|---------|
| Coverage | **Complete.** 9 primary variants + 1 catch-all (`Route`), covering all features listed in gaps.md and README. |
| Error codes | All `QQL-EDGE-UNSUPPORTED-*` prefixed, stable, and unique (tested at `unsupported.rs:142-186`). |
| Message structure | Every variant has `why()` + `remote_hint()` (where applicable) + `message()`. |
| README alignment | The table in `README.md:126-136` exactly matches all variants. |
| Cross-reference in code | Each variant is properly used in mod.rs: `GroupBy` (line 188), `ShardRouting` (line 190), `CollectionSharding` (line 491), `AlterCollection` (line 520), `CollectionParams` (line 497), `Acorn` (line 468), `RecommendAverageVector` (line 123-125), `PointReferenceQuery` (line 284), `Route` (line 646-653), `ShardKeyDdl` (lines 550-551). |

**Only observation**: MMR-with-sparse rejection is handled as a runtime error (`QQL-EDGE-QUERY`), not through this catalog. This is consistent with the catalog's scope (product features, not operational errors).

**Confidence: confirmed**

---

### 7. Cargo.toml Unused Dependencies

| Dependency | Status | Notes |
|------------|--------|-------|
| `qql-core` | **Used** | `QqlError` referenced in every backend file + embedder + lib |
| `qql-plan` | **Used** | Routing, plan types, point IDs in all backend files |
| `qql-embed` | **Used** | `Embedder` trait, `SparseVector` in embedder.rs |
| `qql` (qql-runtime) | **Used** | `Executor`, `QqlConfig`, `HttpEmbedderOptions`, `HttpEmbedder`, `QdrantOps`, etc. |
| `fastembed` | **Used** | All model types, `TextEmbedding`, `ImageEmbedding`, `Bgem3Embedding`, `TextRerank` |
| `qdrant-edge` | **Used** | `EdgeShard`, `EdgeConfigBuilder`, query types, vector types |
| `tokio` | **Used** | `Mutex`, `RwLock` (mod.rs), `spawn_blocking` (embedder.rs + mod.rs) |
| `async-trait` | **Used** | `#[async_trait]` on `impl Embedder for FastEmbedder` + `impl QdrantOps for EdgeQdrant` |
| `serde` (with `"derive"`) | **PARTIALLY UNUSED** | The `serde` crate is used (for `serde::Serialize` trait bound on lines 539, 1074). **But the `"derive"` feature is not used** — no type in qql-edge derives `Serialize` or `Deserialize`. |
| `serde_json` | **Used** | `Value`, `json!` macro, `from_value`, `to_value` — used pervasively |
| `uuid` | **Used** | `uuid::Uuid::parse_str` in `conversions.rs:14` |

**Finding**: `serde` dependency can drop `"derive"` feature. It may be pulled in transitively from qql-core (which enables `features = ["serde", ...]`), but the explicit dependency in qql-edge's Cargo.toml specifies `features = ["derive"]` which is unused by this crate.

**Confidence: confirmed**

---

### 8. Architecture Issues

| Issue | Location | Details |
|-------|----------|---------|
| **Duplicate filter conversion logic** | `mod.rs:1073-1097` and `query_converter.rs:539-551` | Both `convert_edge_filter` and `convert_filter` serialize a filter value, wrap bare conditions in `{"must": [...]}`, and deserialize into `qdrant_edge::Filter`. **Identical logic, different error message prefixes** (`"QQL-EDGE-FILTER-*"` vs `"QQL-EDGE-QUERY"`). Should be unified. |
| **`image_dimension` missing from `Embedder` trait** | `embedder.rs:716-717` (comment) | `FastEmbedder` has `image_dimension()` as an inherent method but the `Embedder` trait (defined in `qql-embed`) has no such method. The config builder in `lib.rs:120` calls it directly on the concrete `FastEmbedder` type, so it works — but generic code can't query image dimensions. |
| **`reject_shard` wrapper in mod.rs** | `mod.rs:1010-1012` | Thin wrapper: `fn reject_shard(shard_key: Option<&str>) -> Result<(), QqlError> { reject_shard_key(shard_key) }`. This just calls the already-public `unsupported::reject_shard_key`. Adds indirection without behavioral change. |
| **`execute_edge` is a 488-line match** | `mod.rs:168-656` | Single match on `RequestBody` variants handles ~14 different operations plus `None` routes. The SQL-style operation handling is colocated by variant, but the function is long. |
| **Double-checked locking is correct but dense** | `mod.rs:112-133` | `open_shard_with_req` acquires the shards read lock twice with an opening mutex in between. This is correct (prevents duplicate initialization) but the pattern repeats the read lock check three times (lines 113-117, 129-133, and after opening). |
| **`HashMap<String, Arc<Mutex<()>>>` for opening guard** | `mod.rs:43, 119-126` | `opening` is a `Mutex<HashMap<String, Arc<Mutex<()>>>>` — a per-collection initialization mutex stored in a map behind a global mutex. Works correctly but is an unconventional pattern. Arguably could use a `tokio::sync::Semaphore` or `once_cell`. |
| **`SparseVector` unused import in lib.rs test?** | `lib.rs:280` | Actually used in the `TestEmbedder` impl at line 298 — confirmed used. |

**Confidence: confirmed** for all items.

---

### Summary of Actionable Findings

| # | Severity | Finding | File:Line |
|---|----------|---------|-----------|
| 1 | Minor | `serde` `"derive"` feature unused | `Cargo.toml:39` |
| 2 | Minor | `resolve_reranker_model` not re-exported from lib.rs | `embedder.rs:635` vs `lib.rs:38-41` |
| 3 | Minor | `list_embedding_models` doc omits image models | `embedder.rs:487-488` |
| 4 | Minor | Duplicate filter conversion logic | `mod.rs:1073-1097` and `query_converter.rs:539-551` |
| 5 | Minor | `image_dimension` not on `Embedder` trait (acknowledged) | `embedder.rs:716-717` |
| 6 | Info | `reject_shard` thin wrapper is redundant | `mod.rs:1010-1012` |
| 7 | Info | `fastembed-rs.md` sparse/Qwen3 models not available offline | — |
| 8 | Info | No TODO/FIXME/HACK comments (clean codebase) | — |
| 9 | Info | gaps.md capability matrix fully matches code | — |
| 10 | Info | unsupported.rs catalog is complete and tested | `unsupported.rs` |
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Deep-dive openapi and proto",
  "subagent_type": "rust-explore",
  "prompt": "Read these files thoroughly:\n1. /data/codebases/qql-rs/crates/qql-runtime/openapi.json - Read in chunks (it's likely large). Focus on request/response schemas for search, query, upsert, scroll, recommend, discover, orderby, fusion, formula, hybrid, rerank, group, count, delete, update, create collection, alter collection, create index.\n2. /data/codebases/qql-rs/crates/qql-runtime/proto/points.proto\n3. /data/codebases/qql-rs/crates/qql-runtime/proto/collections.proto\n4. /data/codebases/qql-rs/crates/qql-runtime/proto/qdrant.proto\n5. /data/codebases/qql-rs/crates/qql-runtime/proto/points_service.proto\n6. /data/codebases/qql-rs/crates/qql-runtime/proto/collections_service.proto\n\nReport:\n- The key endpoints and their request/response shapes\n- Any fields that seem important but might be missing from QQL coverage\n- The SearchParams/quantization/recommend/discover/orderby schemas in detail\n- GroupBy request/response shape\n- Any request-level parameters like timeout, shard_key_selector, read_consistency\n- The exact field names and types used in the proto/openapi"
}
```

**Output:**
```
<task id="ses_0523a32e8ffee9KtaSZYESOloE" state="completed">
<task_result>
Now I have all the data. Here is the comprehensive report:

---

## Complete API Schema Report: Qdrant REST (OpenAPI) + gRPC (Proto)

### 1. KEY ENDPOINTS AND REQUEST/RESPONSE SHAPES

#### REST Endpoints (via OpenAPI)

| Endpoint | Method | Request Schema | Response `result` | Deprecated |
|---|---|---|---|---|
| `/collections/{collection_name}` | **GET** | (path+query params only) | `CollectionInfo` | No |
| `/collections/{collection_name}` | **POST** | `CreateCollection` | `boolean` | No |
| `/collections/{collection_name}` | **PATCH** | `UpdateCollection` | `boolean` | No |
| `/collections/{collection_name}` | **DELETE** | (path+query params only) | `boolean` | No |
| `/collections/{collection_name}/points` | **PUT** | `PointInsertOperations` → `PointsList \| PointsBatch` | `UpdateResult` | No |
| `/collections/{collection_name}/points/delete` | **POST** | `PointsSelector` | `UpdateResult` | No |
| `/collections/{collection_name}/points/scroll` | **POST** | `ScrollRequest` | `ScrollResult` | No |
| `/collections/{collection_name}/points/search` | **POST** | `SearchRequest` | `ScoredPoint[]` | **Yes** |
| `/collections/{collection_name}/points/search/batch` | **POST** | `SearchRequestBatch` | `BatchResult[]` | **Yes** |
| `/collections/{collection_name}/points/search/groups` | **POST** | `SearchGroupsRequest` | `GroupsResult` | **Yes** |
| `/collections/{collection_name}/points/recommend` | **POST** | `RecommendRequest` | `ScoredPoint[]` | **Yes** |
| `/collections/{collection_name}/points/discover` | **POST** | `DiscoverRequest` | `ScoredPoint[]` | **Yes** |
| `/collections/{collection_name}/points/count` | **POST** | `CountRequest` | `CountResult` | No |
| `/collections/{collection_name}/points/query` | **POST** | `QueryRequest` | `QueryResponse` | **No** (universal) |
| `/collections/{collection_name}/points/query/batch` | **POST** | `QueryRequestBatch` | `QueryResponse[]` | No |
| `/collections/{collection_name}/points/query/groups` | **POST** | `QueryGroupsRequest` | `GroupsResult` | No |
| `/collections/{collection_name}/points/facet` | **POST** | `FacetRequest` | `FacetResponse` | No |
| `/collections/{collection_name}/points/search/matrix/offsets` | **POST** | `SearchMatrixRequest` | `SearchMatrixOffsetsResponse` | No |
| `/collections/{collection_name}/points/search/matrix/pairs` | **POST** | `SearchMatrixRequest` | `SearchMatrixPairsResponse` | No |

#### gRPC Service Endpoints (via `points_service.proto` + `collections_service.proto`)

| RPC | Request | Response |
|---|---|---|
| `Upsert` | `UpsertPoints` | `PointsOperationResponse` |
| `Delete` | `DeletePoints` | `PointsOperationResponse` |
| `Get` | `GetPoints` | `GetResponse` |
| `UpdateVectors` | `UpdatePointVectors` | `PointsOperationResponse` |
| `DeleteVectors` | `DeletePointVectors` | `PointsOperationResponse` |
| `SetPayload` | `SetPayloadPoints` | `PointsOperationResponse` |
| `OverwritePayload` | `SetPayloadPoints` | `PointsOperationResponse` |
| `DeletePayload` | `DeletePayloadPoints` | `PointsOperationResponse` |
| `ClearPayload` | `ClearPayloadPoints` | `PointsOperationResponse` |
| `CreateFieldIndex` | `CreateFieldIndexCollection` | `PointsOperationResponse` |
| `DeleteFieldIndex` | `DeleteFieldIndexCollection` | `PointsOperationResponse` |
| `CreateVectorName` | `CreateVectorNameRequest` | `PointsOperationResponse` |
| `DeleteVectorName` | `DeleteVectorNameRequest` | `PointsOperationResponse` |
| `Search` | `SearchPoints` | `SearchResponse` |
| `SearchBatch` | `SearchBatchPoints` | `SearchBatchResponse` |
| `SearchGroups` | `SearchPointGroups` | `SearchGroupsResponse` |
| `Scroll` | `ScrollPoints` | `ScrollResponse` |
| `Recommend` | `RecommendPoints` | `RecommendResponse` |
| `RecommendBatch` | `RecommendBatchPoints` | `RecommendBatchResponse` |
| `RecommendGroups` | `RecommendPointGroups` | `RecommendGroupsResponse` |
| `Discover` | `DiscoverPoints` | `DiscoverResponse` |
| `DiscoverBatch` | `DiscoverBatchPoints` | `DiscoverBatchResponse` |
| `Count` | `CountPoints` | `CountResponse` |
| `UpdateBatch` | `UpdateBatchPoints` | `UpdateBatchResponse` |
| `Query` | `QueryPoints` | `QueryResponse` |
| `QueryBatch` | `QueryBatchPoints` | `QueryBatchResponse` |
| `QueryGroups` | `QueryPointGroups` | `QueryGroupsResponse` |
| `Facet` | `FacetCounts` | `FacetResponse` |
| `SearchMatrixPairs` | `SearchMatrixPoints` | `SearchMatrixPairsResponse` |
| `SearchMatrixOffsets` | `SearchMatrixPoints` | `SearchMatrixOffsetsResponse` |
| `Collections.Create` | `CreateCollection` | `CollectionOperationResponse` |
| `Collections.Update` | `UpdateCollection` | `CollectionOperationResponse` |
| `Collections.Delete` | `DeleteCollection` | `CollectionOperationResponse` |
| `Collections.Get` | `GetCollectionInfoRequest` | `GetCollectionInfoResponse` |
| `Collections.List` | `ListCollectionsRequest` | `ListCollectionsResponse` |
| `Collections.CollectionExists` | `CollectionExistsRequest` | `CollectionExistsResponse` |
| `Collections.CollectionClusterInfo` | `CollectionClusterInfoRequest` | `CollectionClusterInfoResponse` |
| `Collections.UpdateAliases` | `ChangeAliases` | `CollectionOperationResponse` |
| `Collections.CreateShardKey` | `CreateShardKeyRequest` | `CreateShardKeyResponse` |
| `Collections.DeleteShardKey` | `DeleteShardKeyRequest` | `DeleteShardKeyResponse` |
| `Collections.ListShardKeys` | `ListShardKeysRequest` | `ListShardKeysResponse` |

---

### 2. SearchParams (Proto: line 503-520, OpenAPI: line 9882-9925)

```protobuf
message SearchParams {
  optional uint64 hnsw_ef = 1;       // HNSW beam size
  optional bool exact = 2;            // Exact search (no approximation)
  optional QuantizationSearchParams quantization = 3;
  optional bool indexed_only = 4;     // Only indexed/small segments
  optional AcornSearchParams acorn = 5;
}
```

**OpenAPI field names match** (`hnsw_ef`, `exact`, `quantization`, `indexed_only`, `acorn`).

### QuantizationSearchParams (Proto: line 466-483, OpenAPI: line 9927-9948)

```protobuf
message QuantizationSearchParams {
  optional bool ignore = 1;          // Skip quantized data
  optional bool rescore = 2;         // Rescore with original vectors
  optional double oversampling = 3;  // Oversampling factor (>=1.0)
}
```

### AcornSearchParams (Proto: line 485-501, OpenAPI: line 9950-9967)

```protobuf
message AcornSearchParams {
  optional bool enable = 1;           // Enable ACORN for HNSW
  optional double max_selectivity = 2; // Max filter selectivity (0.0-1.0, default 0.4)
}
```

---

### 3. RecommendRequest / RecommendPointGroups (Proto)

**RecommendPoints** (Proto line 680-719):
```
string collection_name         // required
repeated PointId positive      // positive point IDs
repeated PointId negative      // negative point IDs
Filter filter
uint64 limit                   // required
WithPayloadSelector with_payload
SearchParams params
optional float score_threshold
optional uint64 offset
optional string using          // vector name
optional WithVectorsSelector with_vectors
optional LookupLocation lookup_from
optional ReadConsistency read_consistency
optional RecommendStrategy strategy    // AverageVector | BestScore | SumScores
repeated Vector positive_vectors       // inline positive vectors
repeated Vector negative_vectors       // inline negative vectors
optional uint64 timeout
optional ShardKeySelector shard_key_selector
```

**RecommendInput** (Proto line 852-859) — used in the `Query` variant:
```
repeated VectorInput positive
repeated VectorInput negative
optional RecommendStrategy strategy
```

**OpenAPI `RecommendRequest`** (line 10067-10199):
```
shard_key      ShardKeySelector
positive       RecommendExample[]      (PointId | float[] dense | SparseVector)
negative       RecommendExample[]
strategy       RecommendStrategy       ("average_vector" | "best_score" | "sum_scores")
filter         Filter
params         SearchParams
limit          uint (required)
offset         uint (nullable)
with_payload   WithPayloadInterface
with_vector    WithVector
score_threshold float (nullable)
using          string (nullable)
lookup_from    LookupLocation (nullable)
```

---

### 4. DiscoverRequest / DiscoverPoints (Proto)

**DiscoverPoints** (Proto line 796-825):
```
string collection_name
TargetVector target               // VectorExample { PointId id | Vector vector }
repeated ContextExamplePair context // pairs of {positive, negative}
Filter filter
uint64 limit
WithPayloadSelector with_payload
SearchParams params
optional uint64 offset
optional string using
optional WithVectorsSelector with_vectors
optional LookupLocation lookup_from
optional ReadConsistency read_consistency
optional uint64 timeout
optional ShardKeySelector shard_key_selector
```

**DiscoverInput** (Proto line 868-873) — used in `Query`:
```
VectorInput target
ContextInput context              // repeated ContextInputPair {positive VectorInput, negative VectorInput}
```

**OpenAPI `DiscoverRequest`** (line 15612-15729):
```
shard_key      ShardKeySelector
target         RecommendExample (nullable)
context        ContextExamplePair[] (nullable)
filter         Filter
params         SearchParams
limit          uint (required)
offset         uint (nullable)
with_payload   WithPayloadInterface
with_vector    WithVector
using          string (nullable)
lookup_from    LookupLocation (nullable)
```

**OpenAPI `ContextExamplePair`** (line 15732-15745):
```
positive       RecommendExample (required)
negative       RecommendExample (required)
```

---

### 5. OrderBy (Proto line 623-630, OpenAPI line 10346-10378)

```protobuf
message OrderBy {
  string key = 1;              // Payload key
  optional Direction direction = 2;  // Asc (0) | Desc (1)
  optional StartFrom start_from = 3;
}

message StartFrom {
  oneof value {
    double float = 1;
    int64 integer = 2;
    google.protobuf.Timestamp timestamp = 3;
    string datetime = 4;
  }
}
```

**OpenAPI `OrderByInterface`** (line 10336-10344) also accepts plain **string** (key name).

---

### 6. GroupBy — All Group Request/Response Shapes

#### SearchPointGroups (Proto line 573-607)
```protobuf
message SearchPointGroups {
  string collection_name = 1;
  repeated float vector = 2;
  Filter filter = 3;
  uint32 limit = 4;             // max groups
  WithPayloadSelector with_payload = 5;
  SearchParams params = 6;
  optional float score_threshold = 7;
  optional string vector_name = 8;
  optional WithVectorsSelector with_vectors = 9;
  string group_by = 10;         // payload field (required)
  uint32 group_size = 11;       // points per group (required)
  optional ReadConsistency read_consistency = 12;
  optional WithLookup with_lookup = 13;
  optional uint64 timeout = 14;
  optional ShardKeySelector shard_key_selector = 15;
  optional SparseIndices sparse_indices = 16;
}
```

#### RecommendPointGroups (Proto line 731-774)
Same fields as `RecommendPoints` plus:
```
string group_by = 12;
uint32 group_size = 13;
optional WithLookup with_lookup = 15;
```

#### QueryPointGroups (Proto line 1143-1183)
```
repeated PrefetchQuery prefetch = 2;
optional Query query = 3;
optional string using = 4;
optional Filter filter = 5;
optional SearchParams params = 6;
optional float score_threshold = 7;
WithPayloadSelector with_payload = 8;
optional WithVectorsSelector with_vectors = 9;
optional LookupLocation lookup_from = 10;
optional uint64 limit = 11;       // max groups
optional uint64 group_size = 12;  // points per group
string group_by = 13;             // required
optional ReadConsistency read_consistency = 14;
optional WithLookup with_lookup = 15;
optional uint64 timeout = 16;
optional ShardKeySelector shard_key_selector = 17;
```

#### GroupsResult / PointGroup (Proto line 1424-1436)
```protobuf
message PointGroup {
  GroupId id = 1;                    // unsigned | integer | string
  repeated ScoredPoint hits = 2;
  RetrievedPoint lookup = 3;         // from lookup collection
}
message GroupsResult {
  repeated PointGroup groups = 1;
}
```

**OpenAPI `PointGroup`** (line 15074-15102):
```
id       GroupId        (uint64 | int64 | string)
hits     ScoredPoint[]  (points in group)
lookup   Record|null    (lookup record)
```

**OpenAPI `SearchGroupsRequest`** (line 15121-15200):
```
shard_key       ShardKeySelector
vector          NamedVectorStruct (required)
filter          Filter
params          SearchParams
with_payload    WithPayloadInterface
with_vector     WithVector
score_threshold float (nullable)
group_by        string (required)
group_size      uint32 (required)
limit           uint32 (required, max groups)
with_lookup     WithLookupInterface (nullable)
```

**OpenAPI `QueryGroupsRequest`** (line 16796-16937):
```
shard_key       ShardKeySelector
prefetch        Prefetch|Prefetch[] (nullable)
query           QueryInterface (nullable)
using           string (nullable)
filter          Filter
params          SearchParams
score_threshold float (nullable)
with_vector     WithVector
with_payload    WithPayloadInterface
lookup_from     LookupLocation
group_by        string (required)
group_size      uint (nullable)
limit           uint (nullable)
with_lookup     WithLookupInterface (nullable)
```

---

### 7. The `Query` unified variant (Proto line 1047-1072)

The `Query` message is a **oneof** with these variants:
```
nearest            VectorInput
recommend          RecommendInput
discover           DiscoverInput
context            ContextInput
order_by           OrderBy
fusion             Fusion (RRF|DBSF)
sample             Sample (Random)
formula            Formula
nearest_with_mmr   NearestInputWithMmr (nearest + Mmr params)
rrf                Rrf (k + weights)
relevance_feedback RelevanceFeedbackInput
```

**OpenAPI `Query`** (line 16049-16081) mirrors this exactly with `NearestQuery`, `RecommendQuery`, `DiscoverQuery`, `ContextQuery`, `OrderByQuery`, `FusionQuery`, `RrfQuery`, `FormulaQuery`, `SampleQuery`, `RelevanceFeedbackQuery`.

---

### 8. `PrefetchQuery` / `Prefetch` (Proto line 1074-1095)

```protobuf
message PrefetchQuery {
  repeated PrefetchQuery prefetch = 1;  // nested sub-requests
  optional Query query = 2;
  optional string using = 3;
  optional Filter filter = 4;
  optional SearchParams params = 5;
  optional float score_threshold = 6;
  optional uint64 limit = 7;
  optional LookupLocation lookup_from = 8;
}
```

**OpenAPI `Prefetch`** (line 15917-16001) is identical.

---

### 9. `Formula` and `Expression` (Proto line 923-1006)

```protobuf
message Formula {
  Expression expression = 1;
  map<string, Value> defaults = 2;
}

message Expression {
  oneof variant {
    float constant = 1;
    string variable = 2;         // payload key or "score"
    Condition condition = 3;     // 1.0 if true, 0.0 if false
    GeoDistance geo_distance = 4;
    string datetime = 5;         // date-time constant
    string datetime_key = 6;     // payload key with date-time
    MultExpression mult = 7;
    SumExpression sum = 8;
    DivExpression div = 9;
    Expression neg = 10;
    Expression abs = 11;
    Expression sqrt = 12;
    PowExpression pow = 13;
    Expression exp = 14;
    Expression log10 = 15;
    Expression ln = 16;
    DecayParamsExpression exp_decay = 17;
    DecayParamsExpression gauss_decay = 18;
    DecayParamsExpression lin_decay = 19;
  }
}
```

**OpenAPI `Expression`** (line 16332-16392) matches exactly (uses named wrappers like `GeoDistance`, `MultExpression`, etc.).

---

### 10. Fusion and RRF (Proto)

```protobuf
enum Fusion { RRF = 0; DBSF = 1; }

message Rrf {
  optional uint32 k = 1;
  repeated float weights = 2;      // per-prefetch weights
}
```

---

### 11. NearestInputWithMmr and Mmr (Proto line 1008-1033)

```protobuf
message NearestInputWithMmr {
  VectorInput nearest = 1;
  Mmr mmr = 2;
}
message Mmr {
  optional float diversity = 2;      // [0,1], default 0.5
  optional uint32 candidates_limit = 3;  // max candidates for reranking
}
```

---

### 12. Request-Level Parameters

#### Timeout — appears on *every* endpoint
| Field | Type | Where |
|---|---|---|
| `timeout` | `uint64` (seconds) | Proto: optional on every request. OpenAPI: query param on every endpoint |

#### ReadConsistency
```protobuf
message ReadConsistency {
  oneof value {
    ReadConsistencyType type = 1;  // All=0, Majority=1, Quorum=2
    uint64 factor = 2;             // exact node count
  }
}
```
**OpenAPI**: `consistency` query param on all read endpoints (search, scroll, recommend, discover, count, query, get).

#### ShardKeySelector
```protobuf
message ShardKeySelector {
  repeated ShardKey shard_keys = 1;
  optional ShardKey fallback = 2;
}
message ShardKey {
  oneof key {
    string keyword = 1;
    uint64 number = 2;
  }
}
```
**OpenAPI**: `shard_key` field in every request body (as `ShardKeySelector` or `ShardKey`).

#### WriteOrdering
```protobuf
message WriteOrdering {
  WriteOrderingType type = 1;   // Weak=0, Medium=1, Strong=2
}
```
**Proto only** — appears on `UpsertPoints`, `DeletePoints`, `UpdatePointVectors`, `DeletePointVectors`, `SetPayloadPoints`, `DeletePayloadPoints`, `ClearPayloadPoints`.

**OpenAPI**: `ordering` query param on upsert.

#### Wait
`optional bool wait` — Proto only, on all write operations.

---

### 13. CreateCollection Schema (Proto line 468-507, OpenAPI line 10458-10582)

| Field | Type | Description |
|---|---|---|
| `collection_name` | `string` (required) | Name of the collection |
| `vectors` | `VectorsConfig` (optional) | Vector params (dense) |
| `sparse_vectors` | `SparseVectorConfig` (optional) | Sparse vector params |
| `shard_number` | `uint32` (optional) | Number of shards |
| `sharding_method` | `ShardingMethod` (optional) | `Auto` or `Custom` |
| `replication_factor` | `uint32` (optional) | Replicas per shard |
| `write_consistency_factor` | `uint32` (optional) | Min successful writes |
| `on_disk_payload` | `bool` (optional) | Payload on disk |
| `hnsw_config` | `HnswConfigDiff` (optional) | HNSW params |
| `wal_config` | `WalConfigDiff` (optional) | WAL config |
| `optimizers_config` | `OptimizersConfigDiff` (optional) | Optimizer params |
| `quantization_config` | `QuantizationConfig` (optional) | Quantization |
| `strict_mode_config` | `StrictModeConfig` (optional) | Strict mode |
| `metadata` | `map<string, Value>` (optional) | Arbitrary JSON metadata |

**OpenAPI** uses `vectors` (single `VectorsConfig`), but **Proto** has both `vectors_config` and `sparse_vectors_config`.

### 14. UpdateCollection Schema (Proto line 509-534, OpenAPI line 10889-10981)

| Field | Type | Description |
|---|---|---|
| `vectors` | `VectorsConfigDiff` (optional) | Update vector params |
| `sparse_vectors` | `SparseVectorConfig` (optional) | Update sparse params |
| `optimizers_config` | `OptimizersConfigDiff` (optional) | Optimizer changes |
| `params` | `CollectionParamsDiff` (optional) | Base params changes |
| `hnsw_config` | `HnswConfigDiff` (optional) | HNSW changes |
| `quantization_config` | `QuantizationConfigDiff` (optional) | Quantization changes |
| `strict_mode_config` | `StrictModeConfig` (optional) | Strict mode changes |
| `metadata` | `map<string, Value>` (optional) | Merge metadata |

### 15. CreateFieldIndex (Proto line 325-340)

```protobuf
message CreateFieldIndexCollection {
  string collection_name = 1;
  optional bool wait = 2;
  string field_name = 3;              // required
  optional FieldType field_type = 4;   // Keyword | Integer | Float | Geo | Text | Bool | Datetime | Uuid
  optional PayloadIndexParams field_index_params = 5;
  optional WriteOrdering ordering = 6;
  optional uint64 timeout = 7;
}
```

### 16. Quantization Config Types (Proto collections.proto)

**ScalarQuantization**: `type` (Int8), `quantile`, `always_ram`
**ProductQuantization**: `compression` (x4/x8/x16/x32/x64), `always_ram`
**BinaryQuantization**: `always_ram`, `encoding` (OneBit/TwoBits/OneAndHalfBits), `query_encoding`
**TurboQuantization**: `always_ram`, `bits` (1/1.5/2/4)

---

### 17. Fields That Might Be Missing From QQL Coverage

These are fields/features present in the API that may be missing or underutilized in QQL:

| Field | Location | Notes |
|---|---|---|
| `update_filter` | `UpsertPoints` / `PointStructList` | Filter only matching points during upsert |
| `update_mode` | `UpsertPoints` | `Upsert` vs `InsertOnly` vs `UpdateOnly` |
| `update_filter` | `UpdatePointVectors` | Filter for vector updates |
| `ReadConsistency.factor` | `ReadConsistency` | Numeric node count (not just All/Majority/Quorum) |
| `ShardKeySelector.fallback` | `ShardKeySelector` | Fallback shard key |
| `WriteOrdering.type` | `WriteOrdering` | Weak/Medium/Strong ordering guarantees |
| `start_from` | `OrderBy` | Start scrolling from a specific value |
| `mmr` (in `nearest_with_mmr`) | `Query` | Maximal Marginal Relevance reranking |
| `rrf.weights` | `Rrf` | Per-prefetch weights |
| `formula.defaults` | `Formula` | Default values for formula variables |
| `relevance_feedback` | `Query` | Relevance feedback query type |
| `search_matrix` | `SearchMatrixPoints` | Distance matrix computation |
| `facet` | `FacetCounts` | Facet/pivot counting |
| `oversampling` | `QuantizationSearchParams` | Oversampling factor for quantization |
| `acorn` (in `SearchParams`) | `AcornSearchParams` | ACORN HNSW optimization |
| `sparse_indices` | `SearchPoints`, `SearchPointGroups` | Sparse vector indices in legacy endpoints |
| `with_lookup` | All groups endpoints | Cross-collection lookup via group ids |
| `LookupLocation.shard_key` | `LookupLocation` | Shard key selection for lookup |
| `multivector_config` | `DenseVectorCreationConfig`, `VectorParams` | Multi-vector (ColBERT) config |
| `Datatype` | `VectorParams`, `DenseVectorCreationConfig` | Float32/Uint8/Float16 |
| `PrefetchQuery` nested prefetch | `PrefetchQuery` | Arbitrary nesting of prefetches |
| `BinaryQuantization` | Quantization configs | Three encoding variants |
| `TurboQuantization` | Quantization configs | Sub-1-bit quantization |
| `inline_storage` | `HnswConfigDiff` | Store vectors inside HNSW file |
| `payload_m` | `HnswConfigDiff` | Additional payload-aware links |
| `enable_hnsw` | All payload index params | HNSW graph for payload fields |
| `is_principal` | `IntegerIndexParams`, `FloatIndexParams`, `DatetimeIndexParams` | Principal storage key |
| `StopwordsSet` | `TextIndexParams` | Language + custom stopwords |
| `StemmingAlgorithm` (Snowball) | `TextIndexParams` | Per-language stemming |
| `ascii_folding` | `TextIndexParams` | ASCII normalization |
| `phrase_matching` | `TextIndexParams` | Phrase-level text matching |
| `StrictModeConfig` | Create/Update collection | ~20 strict mode knobs |
| `read_fan_out_factor` / `read_fan_out_delay_ms` | `CollectionParamsDiff` | Fan-out read optimization |
| `UpdateBatchPoints` | `PointsService.UpdateBatch` | Batch multiple operations atomically |
| `InferenceObject` / `Document` / `Image` | `VectorInput` | Inference-based vector generation |

---

### 18. Common Shared Types

| Type | Proto Definition | OpenAPI Name |
|---|---|---|
| `PointId` | `oneof { uint64 num; string uuid }` | `ExtendedPointId` |
| `GeoPoint` | `double lon, lat` | `GeoPoint` |
| `Filter` | `repeated Condition should/must/must_not, MinShould min_should` | `Filter` |
| `Condition` | `oneof { FieldCondition, IsEmpty, HasId, Filter nested, IsNull, Nested, HasVector }` | `Condition` |
| `FieldCondition` | `key, Match, Range, GeoBoundingBox, GeoRadius, ValuesCount, GeoPolygon, DatetimeRange, is_empty, is_null` | `FieldCondition` |
| `Match` | `oneof { keyword, integer, boolean, text, keywords, integers, except_* }` | `Match` |
| `Range` | `optional double lt/gt/gte/lte` | `Range` |
| `Value` | `oneof { null, double, integer, string, bool, Struct, ListValue }` | `Payload` (any JSON value) |
| `ScoredPoint` | `PointId id, map payload, float score, uint64 version, VectorsOutput vectors, ShardKey shard_key, OrderValue order_value` | `ScoredPoint` |
| `Record` (RetrievedPoint) | `PointId id, map payload, VectorsOutput vectors, ShardKey shard_key, OrderValue order_value` | `Record` |
| `VectorInput` | `oneof { PointId id, DenseVector, SparseVector, MultiDenseVector, Document, Image, InferenceObject }` | `VectorInput` |
| `Vectors` | `oneof { Vector, NamedVectors }` | `VectorStruct` |
| `VectorsOutput` | `oneof { VectorOutput, NamedVectorsOutput }` | `VectorStructOutput` |
| `WithPayloadSelector` | `oneof { bool enable, PayloadIncludeSelector, PayloadExcludeSelector }` | `WithPayloadInterface` |
| `WithVectorsSelector` | `oneof { bool enable, VectorsSelector include }` | `WithVector` |

---

**Confidence: Confirmed** — All field names and types verified directly from the source files at the cited line numbers.
</task_result>
</task>
```

---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 9.7s)

_Thinking:_

Excellent! The subagents have returned very comprehensive reports. Now let me consolidate and cross-reference these findings, check the actual contract_test.rs and other specific files mentioned, and compile a master report. Let me also check some specific areas mentioned by the subagents to verify findings.


---

## Assistant (Rust-Engineer · DeepSeek V4 Pro · 184.9s)

_Thinking:_

Let me now quickly check the binding crates and see if there's any stale code there. Also check the docs for syntax that may reference removed features.

**Tool: task**

**Input:**
```json
{
  "description": "Audit bindings and cli crates",
  "prompt": "Conduct a thorough audit of these crates in the qql-rs workspace. Find dead code, half-baked features, documentation issues, and architectural problems.\n\nCheck each crate's Cargo.toml and all source files for:\n1. Dead code / unused types / unused functions\n2. Half-baked or incomplete features\n3. Stale documentation referencing removed features (SELECT, INSERT, old syntax)\n4. TODO/FIXME/HACK comments\n5. Unused dependencies\n6. Mismatches with the main QQL API (does it expose all QueryExpr variants? Does it handle CrossRerank?)\n7. Architecture issues\n\nCrates to check:\n- /data/codebases/qql-rs/crates/qql-cli/ (CLI tool)\n- /data/codebases/qql-rs/crates/pyqql/ (Python bindings)\n- /data/codebases/qql-rs/crates/nqql/ (Node.js bindings)\n- /data/codebases/qql-rs/crates/qql-wasm/ (WASM bindings)\n\nAlso check:\n- /data/codebases/qql-rs/docs/syntax.md for stale syntax references\n- /data/codebases/qql-rs/docs/STORY.md\n- /data/codebases/qql-rs/docs/filters.md\n- /data/codebases/qql-rs/docs/inject_filter.md\n- /data/codebases/qql-rs/CHANGELOG.md for consistency\n\nReport specific file paths, line numbers, and issue descriptions.",
  "subagent_type": "rust-explore"
}
```

**Output:**
```
<task id="ses_0523693d4ffeYmmhRs4hIyp6Eu" state="completed">
<task_result>
Now I have enough information. Here is the comprehensive audit report.

---

## Comprehensive QQL Workspace Audit

---

### 1. DEAD CODE

#### 1a. `qql-core/src/parser/helpers.rs:27-34` — Dead `parse_optional_vector_name`
The method `parse_optional_vector_name` is annotated `#[allow(dead_code)]` and is never called anywhere in the codebase. It was presumably added for a feature that was never finished or has been removed.

#### 1b. `qql-core/src/parser/syntax.rs:1` — Entire file gated with `#![allow(dead_code)]`
The pest-based `validate_statement` and `validate_script` functions in this file are never called from any production code path. The parser uses `AstLowerer` directly, not the canonical grammar validator. This is dead validation infrastructure.

#### 1c. `qql-cli/src/table.rs:78-81` — `Table::is_empty()`
Marked `#[allow(dead_code)]`. Never called. The `render_report` function checks `report.results.is_empty()` directly instead.

#### 1d. `qql-runtime/src/executor/dml/config.rs:6-22` — `resolve_dense_model` and `resolve_sparse_model`
Both functions are annotated `#[allow(dead_code)]` and are never called. The `DENSE_MODEL_DEFAULT` and `SPARSE_MODEL_DEFAULT` constants (defined in `qql-runtime/src/executor/mod.rs:18-19`) are only referenced from these dead methods.

#### 1e. `qql-runtime/src/executor/dml/config.rs:56-65` — `cloud_model_options`
Also `#[allow(dead_code)]`. Never called.

#### 1f. `qql-runtime/src/grpc_route.rs:282-291` — `vector_params_diff`
Annotated `#[allow(dead_code)]`. This gRPC route function appears to be scaffolding for future `ALTER COLLECTION` gRPC support.

#### 1g. `qql-runtime/src/grpc_route.rs:293-304` — `vectors_config_diff`
Same dead gRPC route scaffolding. Never called.

---

### 2. HALF-BAKED / INCOMPLETE FEATURES

#### 2a. CLI table rendering has no `CROSS_RERANK` handler
**File:** `qql-cli/src/table.rs:277-303`  
The `render_response` function matches on `response.operation` for `"QUERY"`, `"SCROLL"`, `"QUERY_GROUPS"`, `"COUNT"`, `"SHOW_COLLECTIONS"`, `"SHOW_COLLECTION"`. **`"CROSS_RERANK"` is missing**. Cross-encoder rerank results fall through to the generic `_ =>` arm which only prints `response.message` — the actual reranked hit data in `response.data` is never displayed in a table.  
**Impact:** CrossRerank results are invisible in CLI table mode (JSON mode works fine).

#### 2b. `nqql` and `qql-wasm` `route_statement_type` misclassify `DropIndex` and `ListShardKeys`
- **`DropIndex`** (plan.rs:858-863): Gets a `Route` with `Method::Delete`, `body: None`. In `route_statement_type`, this hits `Method::Delete => "drop_collection"` — **incorrect**, should be `"drop_index"`.
- **`ListShardKeys`** (plan.rs:894-899): Gets a `Route` with `Method::Get`, path `/collections/{name}/shards`, `body: None`. Hits `Method::Get => "show_collection"` — **incorrect**, should be `"show_shard_keys"`.
- **`CrossRerank`** (plan.rs:900-904): Gets `Method::Post`, `body: None`. Hits `_ => "unknown"`.

**Files:**
- `crates/nqql/src/lib.rs:190-213` (nqql `compile_query`)
- `crates/qql-wasm/src/lib.rs:524-549` (wasm `route_statement_type`)

#### 2c. `qql-wasm` `wasm_batch_key` mishandles some mutation types
**File:** `crates/qql-wasm/src/lib.rs:155-177`  
The `wasm_statement_batch_key` function treats `ClearPayload` and `DeleteVector` as batchable mutations (grouped under `WasmBatchKey::Mutation`). However, `wasm_planned_batch_key` (line 180-195) uses the plan-level `batch_family()` which classifies these as `BatchFamily::Mutation`. This is correct, but the batch flush logic (line 1117-1253) only handles `PlannedOperation::Query { .. }` and `_` (for mutations). If a `CrossRerank` or `GetCollection` operation gets batched, it falls through to invalid batch handling.

#### 2d. CrossRerank embedder support is incomplete across bindings
WASM `Client` (line 608-617) has no `rerank_endpoint` / `rerank_model` config at all — it only supports dense/sparse embedding. The `Client::execute_stmt_inner` never calls `resolve_stmt_embeddings` for CrossRerank. Cross-encoder pair scoring is not implementable in the WASM client today.

---

### 3. DOCUMENTATION ISSUES

#### 3a. `docs/syntax.md` — No `DropIndex` DDL documentation
The syntax reference documents `create-index`, `drop-index`, `create-shard-key`, `drop-shard-key`, and `show-shard-keys` in the EBNF (lines 443-458), which is correct. However, `SHOW SHARD KEYS` is **not** listed in the initial `statement` production at line 12-13 — the EBNF shows `statement = query | scroll | upsert | update | delete | ddl | count | clear-payload | delete-vectors | create-shard-key ;` — missing `drop-shard-key`, `show-shard-keys`, `drop-index`, etc.

#### 3b. `docs/syntax.md:46` — `MULTIVECTOR` vector kind reference
Line 46 shows `vector-kind = "DENSE" | "SPARSE" | "MULTI" | "MULTIVECTOR"`. The grammar uses `MULTI` and `MULTIVECTOR`, but the doc suggests they are both valid vector kinds. In the AST, only `VectorKind::Multi` exists (aliased to MULTIVECTOR in parsing). This is ambiguous.

#### 3c. `docs/inject_filter.md:199-207` — Operator matrix shows `!=` rejection
This is accurate but misleading: it says "Inequality | `!=`, `neq`, `<>` | ❌ Rejected" with a note to "Wrap equality with NOT (...)". However, `inject_filter` actually returns a `PySyntaxError` if you try to use `!=`. This is a runtime error, not a static rejection. The doc does not clarify the distinction.

#### 3d. `docs/STORY.md` references old `qql` Python history
The story is accurate as history. No stale references to current API.

#### 3e. `CHANGELOG.md` — Missing entries
The CHANGELOG jumps from `[0.1.3]` (July 28) directly to `[Unreleased]`. The `[Unreleased]` section only mentions multivector/ColBERT additions. Many features present in the code (CrossRerank, `CLEAR PAYLOAD`, `DELETE VECTOR`, `SHOW SHARD KEYS`, `DropIndex`) are not reflected in the unreleased changelog.

---

### 4. TODO/FIXME/HACK COMMENTS

There are **no TODO, FIXME, HACK, or XXX comments** anywhere in the crate source files (`crates/**/*.rs`). The codebase is remarkably clean of technical debt markers.

However, there are several `#[allow(dead_code)]` annotations that serve as implicit TODOs (see section 1).

---

### 5. UNUSED DEPENDENCIES

#### 5a. `pyqql/Cargo.toml:22` — `serde` dependency
`serde` is listed as a direct dependency of `pyqql` with `features = ["derive"]`, but `pyqql/src/lib.rs` never uses `serde::Serialize`, `serde::Deserialize`, or any `#[derive(serde::*)]` attribute directly. All serialization goes through `serde_json` and `pythonize`. The dependency is unnecessary — `serde` comes transitively via `qql-core`.

#### 5b. `qql-cli/Cargo.toml:30-31` — `qql-core` and `qql-plan` as direct deps
These are legitimate — `script.rs` uses `qql_core::parser::Parser` and `dump.rs` uses `qql_plan::PlannedOperation`. Not unused.

#### 5c. `qql-wasm/Cargo.toml:35` — `serde` as direct dep
Used indirectly through derive macros. The `#[derive(serde::Serialize)]` on `WasmReport` (line 78) uses it. Not unused.

---

### 6. API EXPOSURE MISMATCHES

#### 6a. `pyqql` exports `compile_query` but `nqql` exports both `compile_query` and `Client::compile`
All bindings expose `compile`/`compile_query`, but the `RequestBody` enum has no variant for `DropIndex` or `ListShardKeys`, so `compile` on these statements falls through to the `None` arm and produces misleading `stmt_type` values (see 2b).

#### 6b. `pyqql` doesn't expose a `compile` method on `PyClient`
**File:** `pyqql/src/lib.rs:387-446`  
`PyClient` has `execute`, `execute_async`, and `explain` but **no `compile` method**. The free function `compile_query` is exported and works, but there's no client-bound variant. Compare with `nqql` where `JsClient::compile` exists (line 448). This is an API inconsistency.

#### 6c. `inject_filter` inconsistencies across bindings
- `pyqql` accepts `query` as `Stmt | str` and returns `PyStmt`
- `nqql` `inject_filter` only accepts `String` (returns JSON), while `Stmt::inject_filter` mutates in-place
- `qql-wasm` `inject_filter` accepts only `String` (returns JS value), while `Stmt::inject_filter` mutates in-place
The `InjectShardKey` function follows the same pattern. These are design differences that are documented but inconsistent.

#### 6d. Missing `CROSS_RERANK` in CLI operation table rendering
See 2a above. Also missing from the `output/table.rs` print path.

---

### 7. ARCHITECTURAL ISSUES

#### 7a. `RequestBody` enum vs. `PlannedOperation` — missing variants for body-less operations
**File:** `crates/qql-plan/src/routing.rs:8-26`
The `RequestBody` enum covers 16 operation types, but 5 `PlannedOperation` variants produce no `RequestBody`:
- `ListCollections` (body: None)
- `GetCollection` (body: None)
- `ListShardKeys` (body: None)
- `DropIndex` (body: None)
- `DropCollection` (body: None)

This is **fine for execution** (the route's method + path carry the information), but creates a brittle pattern in `route_statement_type` which has to match on `method + path` strings. This is error-prone — see 2b. A better architecture would be to either:
(a) Add a `stmt_type` field to `Route` directly, so classification is done once in the planner, or
(b) Add a dedicated `RequestBody::DropIndex` etc. even if the body is unit-like.

#### 7b. CrossRerank planning returns a dummy REST route
**File:** `crates/qql-plan/src/plan.rs:900-904`
`PlannedOperation::CrossRerank` produces `Route { method: Method::Post, path: ".../points/query", body: None }` — this is a lie. CrossRerank is not executable as a single Qdrant request. The route is only used for the `compile()` API which should probably return a special `"client_side"` flag or handle this explicitly rather than returning a misleading path.

#### 7c. `qql-core` is `no_std` but `qql-plan` and `qql-runtime` are not — no clear `no_std` enforcement
The `qql-core` Cargo.toml says it's `no_std` (no `std` feature by default), but `qql-plan` enables `serde` on `qql-core`, and `serde` pulls in `std` features of `serde`. The `no_std` claim is not verified by CI — there is no `cargo check --no-default-features --target x86_64-unknown-none` or similar build step.

#### 7d. Dead methods in `Executor` (`resolve_dense_model`, `resolve_sparse_model`, `cloud_model_options`)
These are vestiges of an older config/inference architecture. Their presence alongside unused constants `DENSE_MODEL_DEFAULT`, `SPARSE_MODEL_DEFAULT`, `INFERENCE_MODE_DEFAULT` (all in `executor/mod.rs:18-23`) suggests a refactoring was started but not completed. The `inference_mode` and `uses_local_embeddings` methods are still used.

#### 7e. gRPC support is feature-gated with `#[cfg]` duplicated in every binding
Every binding crate (pyqql, nqql, qql-cli) has its own `#[cfg(feature = "grpc")]` / `#[cfg(not(feature = "grpc"))]` block. This is 4x duplicated pattern matching for `GrpcQdrant::from_url`. A shared helper function in `qql-runtime` could reduce this.

---

### Summary of Findings by Severity

| Severity | Issue | Location |
|----------|-------|----------|
| **🔴 High** | `DropIndex` compile() returns `"drop_collection"` — misleads callers | `nqql/src/lib.rs:208-211`, `qql-wasm/src/lib.rs:542-546` |
| **🔴 High** | `ListShardKeys` compile() returns `"show_collection"` — misleads callers | Same locations |
| **🟡 Medium** | `CROSS_RERANK` results not tabled in CLI | `qql-cli/src/table.rs:277-303` |
| **🟡 Medium** | CrossRerank returns dummy REST route in compile API | `qql-plan/src/plan.rs:900-904` |
| **🟡 Medium** | `pyqql` `Client` has no `compile` method (unlike `nqql`) | `pyqql/src/lib.rs:387-446` |
| **🟡 Medium** | 5 dead-code items annotated with `#[allow(dead_code)]` | See section 1 |
| **🟡 Medium** | `syntax.md` statement EBNF missing `drop-index`, `drop-shard-key`, `show-shard-keys` | `docs/syntax.md:12-14` |
| **🟡 Medium** | Changelog `[Unreleased]` section incomplete | `CHANGELOG.md:10-18` |
| **🟢 Low** | `pyqql` has unused `serde` direct dependency | `pyqql/Cargo.toml:22` |
| **🟢 Low** | `qql-core` no_std not enforced in CI | No build config |
| **🟢 Low** | `parse_optional_vector_name` dead code | `qql-core/src/parser/helpers.rs:28` |
| **🟢 Low** | `validate_statement` / `validate_script` dead code via pest | `qql-core/src/parser/syntax.rs:1` |
| **🟢 Low** | `vector_params_diff`, `vectors_config_diff` dead gRPC code | `qql-runtime/src/grpc_route.rs:282,293` |
| **🟢 Low** | `resolve_dense_model`, `resolve_sparse_model`, `cloud_model_options` dead | `qql-runtime/src/executor/dml/config.rs:6,24,56` |
</task_result>
</task>
```

**Tool: task**

**Input:**
```json
{
  "description": "Audit remaining edge crates",
  "prompt": "Conduct a thorough audit of these additional crates in the qql-rs workspace:\n\n1. /data/codebases/qql-rs/crates/qql-conformance/ - Read Cargo.toml, src/lib.rs, all source files. Check for dead code, stale tests, unused deps.\n2. /data/codebases/qql-rs/crates/qql-grammar-gen/ - Read Cargo.toml, src/main.rs. What does it do? Is it maintained?\n3. /data/codebases/qql-rs/crates/pyqql-edge/ - Read Cargo.toml, src/lib.rs. Check for dead code, unused deps, stale references.\n4. /data/codebases/qql-rs/crates/nqql-edge/ - Read Cargo.toml, src/lib.rs. Same check.\n5. /data/codebases/qql-rs/crates/qql-runtime/src/executor/mod.rs - Read the full file (offset 0, limit 200, then continue). Look for:\n   - The `unreachable!` at line ~916\n   - The `execute_cross_rerank` method (around line 928-1040) for bugs\n   - The `prepare_statement` method (around line 637-741) for bugs\n6. /data/codebases/qql-rs/crates/qql-runtime/src/backend.rs - Read offset 190-330. Check empty impl block and schema_from_rest_result for bugs.\n7. /data/codebases/qql-rs/crates/qql-runtime/src/grpc_route.rs - Read the `create_collection_raw` (around line 750-820) and `update_collection_raw` (around line 830-870). Check for the datatype: None issue.\n\nReport all findings with file paths, line numbers, and issue descriptions.",
  "subagent_type": "rust-explore"
}
```

**Output:**
```
<task id="ses_052367d96ffelPZDJp9qZH3LeK" state="completed">
<task_result>
Now I have all the information needed. Here is the comprehensive audit report.

---

## Audit Report: Additional Crates in qql-rs Workspace

---

### 1. `qql-conformance` crate

**Location:** `/data/codebases/qql-rs/crates/qql-conformance/`

**Structure:** Single `main.rs` (426 lines), one `Cargo.toml`.

| Check | Finding | Confidence |
|---|---|---|
| **Dead code** | None. All functions (`check`, `generate`, `inspect`, `canonical_ast`, `normalize_numbers`, `invalid_cases`, `qql_files`, `write_atomic`, etc.) are called. | Confirmed |
| **Unused deps** | None. `qql-core`, `qql-plan`, and `serde_json` are all actively used. | Confirmed |
| **Stale tests** | No. The `#[cfg(test)] mod tests` block (lines 372-426) has four tests that test `invalid_cases`, `canonical_ast` versioning, and floating-point rounding. All are relevant and functional. | Confirmed |
| **Minor issues** | `use qql_core::lexer::Lexer` is **not present** (I mistakenly flagged this — re-confirmed it doesn't exist). No imports are unused. | Confirmed |

**Verdict:** Clean, well-maintained, no issues.

---

### 2. `qql-grammar-gen` crate

**Location:** `/data/codebases/qql-rs/crates/qql-grammar-gen/`

**Structure:** Single `main.rs` (101 lines), `Cargo.toml` with **zero dependencies**.

**What it does:**
- A code generator that reads `language/v1/grammar.pest` (the human-authored pest grammar) and writes it as `crates/qql-core/grammar/qql.generated.pest` with a `@generated` header and normalized newlines.
- Two commands: `generate` (write file if changed) and `check` (verify file is current).
- Used in the build process to ensure the generated grammar stays in sync with the source.

**Maintenance status:** Maintained. The code is small, deterministic, has one unit test for newline normalization, and is clearly tied to the CI workflow (the `check` command is for CI gates).

| Check | Finding | Confidence |
|---|---|---|
| **Dead code** | None. | Confirmed |
| **Unused deps** | Zero deps declared. | Confirmed |

**Verdict:** Healthy. No issues.

---

### 3. `pyqql-edge` crate

**Location:** `/data/codebases/qql-rs/crates/pyqql-edge/`

**Structure:** Single `lib.rs` (705 lines), `Cargo.toml`.

**Files:** `src/lib.rs`, `tests/test_parser.py` (323 lines), `tests/test_comprehensive.py` (168 lines).

| Check | Finding | Confidence |
|---|---|---|
| **Dead code** | None identified. All `#[pyfunction]` functions are registered in the `#[pymodule]` init block. All internal helpers (`classify`, `do_explain`, `py_to_value`, `str_to_comparison_op`, `parse_on_error`, etc.) are called. | Confirmed |
| **Unused deps** | None. All 9 dependencies are actively used. | Confirmed |
| **Stale test reference** | `test_comprehensive.py` line 36 expects `__version__` in `dir(pyqql_edge)` exports, but the Rust module (lines 572-593) never defines `__version__`. This test will **always fail** — either the test is stale or `__version__` was never wired up. | **Likely** |
| **Stale `__pycache__`** | `tests/__pycache__/` contains `.pyc` bytecode files from multiple Python versions (3.12, 3.14). These are build artifacts that should be in `.gitignore`. | Confirmed |

**Verdict:** Largely clean, but `test_comprehensive.py` test_a1 has a stale `__version__` expectation, and `__pycache__/` should not be tracked.

---

### 4. `nqql-edge` crate

**Location:** `/data/codebases/qql-rs/crates/nqql-edge/`

**Structure:** Single `lib.rs` (655 lines), `Cargo.toml`.

**Files:** `src/lib.rs` only (no test files).

| Check | Finding | Confidence |
|---|---|---|
| **Dead code** | None. All `#[napi]` functions are exported. `standalone_local_opts` is used by both `execute` variants. `from_executor` is used by constructors. | Confirmed |
| **Unused deps** | None. All 9 dependencies are used. | Confirmed |
| **Feature-duplicated `execute`** | Two `#[napi] pub async fn execute(...)` versions exist — one at line 581 (behind `#[cfg(all(feature = "fastembed-local", not(feature = "http-embedding")))]`) and one at line 603 (behind `#[cfg(feature = "http-embedding")]`). This is intentional to avoid conflicting NAPI exports, but if both features are enabled, the build would fail with a duplicate symbol. The `Cargo.toml` default features only enable `fastembed-local`, so this works in practice, but it's fragile. | Confirmed |

**Verdict:** Clean but the dual `execute` export is a brittle pattern.

---

### 5. `qql-runtime/src/executor/mod.rs` — targeted checks

#### 5a. `unreachable!` at line 916

**File:** `/data/codebases/qql-rs/crates/qql-runtime/src/executor/mod.rs`, line 916

```rust
PlannedOperation::CrossRerank { .. } => unreachable!("handled above"),
```

**Finding:** This lives in `dispatch_planned` (starts line 845). The `CrossRerank` arm is in the `match op` block, but the function has an early `if let PlannedOperation::CrossRerank { ... } = op { return ... }` at lines 852-865 that catches it first.

**Issue:** The `unreachable!` is technically correct **today**, but it's fragile. If someone refactors and removes or reorders the early return, this becomes a runtime panic for a valid input variant. The match arm should either be removed entirely or, if retained as a safety net, should use `Err(...)` instead of `unreachable!`.

**Severity:** Low (defensive concern).

#### 5b. `execute_cross_rerank` method (lines 928-1040)

**File:** `/data/codebases/qql-rs/crates/qql-runtime/src/executor/mod.rs`, lines 928-1040

| Check | Finding | Confidence |
|---|---|---|
| **Dedup semantics** | Uses `HashMap` keyed by `hit.id` (line 948-958) to deduplicate candidates across collections. If the same point ID exists in multiple collections, only the first one encountered is kept. This is a design choice but could lose data silently. | Confirmed |
| **Text field extraction** | Lines 976-983: tries `payload[field]` first, then falls back to `hit.text` (populated by `extract_search_hits` from `payload["text"]`). The `.or(hit.text.as_deref())` is a no-op if `field == "text"` since both read the same source. If `field != "text"`, the fallback to `hit.text` is incorrect — it should be an error or empty string. | **Potential bug** |
| **Empty text early exit** | Lines 990-998: if all candidates have empty text, it returns an error. This is correct but the error only suggests the field name — not which point IDs were problematic. | Minor |
| **Score count mismatch** | Lines 1001-1012: checks `scores.len() != docs.len()` after reranking. Correct guard. | Confirmed |
| **Offset application** | Line 1025: `let skip = offset as usize;` — if `offset` exceeds the ranked list length, `skip` is silently capped (`.skip(skip).take(take)`. This is correct but note that offset is applied *after* ranking, not at the candidate fetch stage. | Confirmed |

**Verdict:** No critical bugs. The `hit.text` fallback when `field != "text"` is suspicious — it means if a user asks for `CROSS RERANK ... FIELD body`, but `hit.text` happens to be populated from `payload["text"]`, the reranker will score against the wrong field content.

#### 5c. `prepare_statement` method (lines 637-741)

**File:** `/data/codebases/qql-rs/crates/qql-runtime/src/executor/mod.rs`, lines 637-741

| Check | Finding | Confidence |
|---|---|---|
| **Redundant Upsert match** | The method first matches `&mut stmt` on `Stmt::Upsert(upsert)` at line 646 to call `configure_upsert_embeddings`, then matches it again at line 659 with `if let Stmt::Upsert(u) = &stmt` for embedding spec aggregation. Both branches operate on the Upsert variant but do different things. **Not a bug**, but consolidating into a single arm would be cleaner. | Minor style |
| **Embedding spec collection** | `collect_specs` (lines 669-696) is a nested fn that recursively traverses `EmbeddingSpec`. It returns empty Vec for `MultiVector` and `Image` variants (line 679), meaning auto-creation of collections with only multivector or image embeddings is skipped. Comment confirms: "MultiVector / Image alone do not auto-create." | Intentional, but limits discoverability |
| **Hybrid embedding aggregation** | Lines 700-733: aggregates dense/sparse flags and models from the spec tree, then calls `ensure_collection_for_upsert`. The logic correctly picks the first dense model, first sparse_* vector name, etc. No bugs found. | Confirmed |
| **Validation** | Lines 735-736: calls `validate_embedded_upsert(u, info)` with the schema from `configure_upsert_embeddings`. Correct. | Confirmed |

**Verdict:** No bugs. Minor style note about the split Upsert handling.

---

### 6. `qql-runtime/src/backend.rs` — targeted checks

**File:** `/data/codebases/qql-rs/crates/qql-runtime/src/backend.rs`

#### 6a. Empty `impl CollectionSchema {}` at line 197

**Finding:** `impl CollectionSchema {}` is an empty block with no methods or trait impls.

**Issue:** Dead code. Either the impl was meant to have methods, or it's left over from a refactor. It compiles silently but adds nothing.

**Severity:** Low (cosmetic).

#### 6b. `schema_from_rest_result` (lines 216-324) — bugs

| Issue | Location | Finding | Severity |
|---|---|---|---|
| **Unnamed vector detection clears named vectors** | Line 230: `schema.dense_vectors.clear()` | If the REST response has both unnamed vectors (`{ "size": N, "distance": "D" }`) and named vectors, the `dense_vectors` Vec (which stores the **names** of named dense vectors) is unconditionally cleared. Named vectors are then never collected. This assumes that if unnamed vectors exist, it's the only config — which matches the Qdrant API spec (mutual exclusivity), but the code processes both branches. | **Low** (only matters if Qdrant API changes) |
| **Payload data_type fallback** | Lines 279-284: `meta.get("data_type")` falls back to `meta.get("type")`, falling back to `"keyword"`. Correct and defensive. | Confirmed |
| **is_tenant lookup** | Lines 294-297: checks `meta.is_tenant` first, then `params.is_tenant`. Correct order. | Confirmed |
| **HNSW config extraction** | Lines 310-313: `filter_hnsw_map` is called on the HNSW config. Correct. | Confirmed |
| **Optimizers config** | Lines 315-319: Handles both `optimizer_config` and `optimizers_config` (singular vs plural) from Qdrant API. Good defensive code. | Confirmed |

**Verdict:** The `dense_vectors.clear()` at line 230 is a minor latent bug if Qdrant ever returns mixed vector configs. Otherwise solid.

---

### 7. `qql-runtime/src/grpc_route.rs` — targeted checks

**File:** `/data/codebases/qql-rs/crates/qql-runtime/src/grpc_route.rs`

#### 7a. `datatype: None` in `vector_params` (line 273)

**Finding:** The `vector_params` function (line 264-280) constructs a `qdrant::VectorParams` protobuf message. At line 273, `datatype: None` is hardcoded — the function never reads a `"datatype"` field from the JSON value.

**Impact:** When users create a collection with a vector configuration specifying `datatype` (e.g., `"uint8"`, `"float16"`), the value is **silently ignored** and the gRPC call sends `None`. The collection gets created with the default datatype. This only affects the gRPC path — the REST path may handle it differently.

**Severity:** **Medium** — silent data loss for users relying on non-default vector datatypes via gRPC.

#### 7b. `datatype: None` in `sparse_vector_params` (line 327)

**Finding:** The `sparse_vector_params` function *does* parse datatype from its JSON (lines 315-323), mapping "float32"/"f32"/"uint8"/"u8"/"float16"/"f16" to the protobuf enum. This is correct.

**Severity:** None for sparse vectors.

#### 7c. Dead code with `#[allow(dead_code)]`

**Location:** Lines 282 (`vector_params_diff`) and 293 (`vectors_config_diff`).

**Finding:** Both functions are marked `#[allow(dead_code)]` and are only referenced in tests (line 2770). They are never called from any production code path in `dispatch_route_planned` or anywhere else.

**Severity:** Low — dead code that was probably intended for future use. Should either be wired up or removed.

---

### Summary Table of All Issues

| # | Crate | File:Line | Issue | Severity |
|---|---|---|---|---|
| 1 | `pyqql-edge` | `tests/test_comprehensive.py:36` | Test expects `__version__` export that doesn't exist | **Medium** (test always fails) |
| 2 | `pyqql-edge` | `tests/__pycache__/` | `.pyc` build artifacts tracked in repo | Low |
| 3 | `nqql-edge` | `lib.rs:581,603` | Two `execute` fns behind mutually exclusive `#[cfg]` — fragile if both features enabled | Low |
| 4 | `qql-runtime` | `executor/mod.rs:916` | `unreachable!` for `CrossRerank` — safe today but brittle | Low |
| 5 | `qql-runtime` | `executor/mod.rs:976-983` | `execute_cross_rerank` falls back to `hit.text` when `payload[field]` missing, which may silently use wrong field | **Medium** |
| 6 | `qql-runtime` | `executor/mod.rs:638-741` | Split `Upsert` handling across two separate match arms (lines 646 and 659) | Minor/style |
| 7 | `qql-runtime` | `backend.rs:197` | Empty `impl CollectionSchema {}` block | Low |
| 8 | `qql-runtime` | `backend.rs:230` | `schema.dense_vectors.clear()` wipes named vector names when unnamed vectors also present | Low |
| 9 | `qql-runtime` | `grpc_route.rs:273` | `datatype: None` hardcoded — vector datatype from JSON **silently ignored** on gRPC path | **Medium** |
| 10 | `qql-runtime` | `grpc_route.rs:282,293` | `#[allow(dead_code)]` on `vector_params_diff` / `vectors_config_diff` — unused in production | Low |
</task_result>
</task>
```

===
🔴 QQL Red Team Audit Report
A comprehensive audit of the qql-rs workspace against the OpenAPI/proto contracts and fastembed-rs.md.
🔴 BUGS — Actual Incorrect Behavior
B1. gRPC silently drops datatype on vector creation
crates/qql-runtime/src/grpc_route.rs:273
datatype: None,  // hardcoded — never reads from JSON
When users create collections via gRPC with a non-default vector datatype (uint8, float16), the setting is silently discarded. The sparse path does handle datatype (line 315-323), but the dense path doesn't. Users get the wrong collection silently.
B2. DropIndex compile() returns misclassified stmt_type = "drop_collection"
crates/nqql/src/lib.rs:208-211, crates/qql-wasm/src/lib.rs:542-546
The route_statement_type matches Method::Delete → "drop_collection", but DropIndex also uses DELETE with body: None. So compile("DROP INDEX ...") returns stmt_type: "drop_collection" — a lie.
B3. ListShardKeys compile() returns misclassified stmt_type = "show_collection"
Same pattern: Method::Get + body: None → "show_collection", but ListShardKeys also uses GET with no body. compile("SHOW SHARD KEYS ...") → stmt_type: "show_collection" — wrong.
B4. CROSS_RERANK field extraction uses wrong fallback text
crates/qql-runtime/src/executor/mod.rs:976-983
let text = payload.get(field).and_then(|v| v.as_str())
    .or(hit.text.as_deref());
If field != "text" (e.g., CROSS RERANK ... FIELD body) and payload["body"] is missing, it falls back to hit.text (which comes from payload["text"]). This means the reranker silently scores against the wrong field content instead of erroring.
B5. CrossRerank REST route is a lie (broken body)
crates/qql-plan/src/plan.rs:900-904
PlannedOperation::CrossRerank { .. } => Route {
    method: Method::Post,
    path: format!("/collections/{collection}/points/query"),
    body: None,   // <--- EMPTY BODY to a POST endpoint!
};
If any code path calls to_rest_route() on a CrossRerank op, it produces an invalid HTTP request. The comment says "never projected as a single Qdrant route" but the code still returns a broken route instead of panicking or returning an error.
B6. RestQdrant::new() panics despite doc saying "constructors never panic"
crates/qql-runtime/src/rest.rs:23-24
pub fn new(...) -> Self {
    Self::with_timeout(...).expect("failed to build reqwest client")  // PANICS
}
Line 27-28 doc says: "Fallible so library constructors never panic (RUN-015 / RUN-010)". The new() convenience wrapper does panic on build failure, contradicting the design doc.
B7. Auto-embedding on UPSERT always adds sparse vector regardless of collection topology
crates/qql-embed/src/resolve.rs:67-101
When there's no explicit embedding directive on an UPSERT, resolve_embeddings always creates both a "dense" vector (via embedder) AND a "sparse" vector (via BM25). If the collection only has dense vectors configured, every point gets an unwanted sparse vector — Qdrant will either reject the upsert or store orphan vectors.
B8. qql-plan Image → Document conversion is lossy
crates/qql-plan/src/semantic.rs:117-121
QueryInput::Image { source, model }
    → PlanQueryInput::Document { text: source, model }
Image source paths/URLs are treated as document text. The distinction between image and text query inputs is lost at the plan layer, so any downstream code cannot know whether the input was an image.
🟡 HALF-BAKED / INCOMPLETE
H1. 3 gRPC trait methods are hard-error stubs
crates/qql-runtime/src/grpc.rs:451-456, 468-473, 476-485
update_collection, create_field_index, delete_field_index all return Err("use execute_route for gRPC"). The DDL analogs work via the planned-operation bypass in execute_planned_grpc(), but anyone calling the QdrantOps trait methods directly gets an error.
H2. qql-plan embedding module (345 lines) is dead code
crates/qql-plan/src/embedding.rs — entire file
extract_jobs() and all associated types (EmbeddingJob, EmbeddingKind, EmbeddingDestination) are never called from any external crate. Only internal self-tests exist. This appears to be a module that was split out to qql-embed but the old extraction logic was left behind.
H3. sparse model selection always rejected at the trait level
crates/qql-embed/src/embedder.rs:25 vs resolve.rs:137-143
The Embedder trait's embed_sparse(text) takes no model parameter. All other embed methods (embed_dense, embed_multi, embed_image, rerank_pairs) take a model: &str. This means even if a host wanted to support SpladePPv1 or BGE-M3 sparse (as fastembed-rs.md documents), they can't express model selection through the trait. The AST supports EMBED SPARSE MODEL '...' but it's always rejected at resolution.
H4. No BGE-M3 joint embedding (dense+sparse+ColBERT in single pass)
crates/qql-embed/src/embedder.rs — Embedder trait
fastembed-rs.md:317-348 explicitly documents Bgem3Embedding producing all three outputs from one forward pass. The Embedder trait has no embed_joint() method — hybrid specs call separate dense and sparse paths. No Qwen3 or Nomic v2 MoE support either (both candle-backend, so this is a feature decision, not a bug).
H5. 5 #[allow(dead_code)] functions in qql-runtime
File	Function	Line
executor/dml/config.rs	resolve_dense_model	6
executor/dml/config.rs	resolve_sparse_model	24
executor/dml/config.rs	cloud_model_options	56
grpc_route.rs	vector_params_diff	282
grpc_route.rs	vectors_config_diff	293
H6. CLI table mode doesn't render CROSS_RERANK results
crates/qql-cli/src/table.rs:277-303 — CROSS_RERANK is missing from the match. Reranked results only appear in JSON mode; table mode shows just the message string.
H7. pyqql Client has no compile() method (unlike nqql)
crates/pyqql/src/lib.rs:387-446 — PyClient has execute, execute_async, explain, but no compile. nqql has JsClient::compile() (line 448). API inconsistency across bindings.
H8. gRPC mutation responses return fake time: 0.0
crates/qql-runtime/src/grpc_route.rs:473-479 — mutation_response() hardcodes "time": 0.0_f64. REST responses carry real server timing. Consumers can't rely on time consistently.
🟢 DEAD CODE / CLEANUP OPPORTUNITIES
D1. pest + pest_derive dependencies for entirely dead module
crates/qql-core/src/parser/syntax.rs:1-63 — The entire file is #![allow(dead_code)]. The validate_statement/validate_script pest-grammar functions are never called from the actual parsing pipeline (which uses the manual AstLowerer). Both pest and pest_derive are ~wasted dependencies in production. Either wire them in or remove them.
D2. SparseEmbedder struct — never instantiated anywhere
crates/qql-embed/src/embedder.rs:175-181 — SparseEmbedder is a public struct with embed_sparse(text) but no code constructs it. Both runtime and edge call qql_embed::sparse::build_query_default(text) directly.
D3. Four dead qql-plan items
Item	File	Line
Method::as_str()	types.rs	20
MinShould struct	types.rs	117
embedding_has_wait()	mutation.rs	160
lower_point_vectors()	mutation.rs	35
MatchValue::TextAny + MatchValue::Except	types.rs	168-176
D4. Three unused runtime dependencies
crates/qql-runtime/Cargo.toml: chrono, uuid, futures-util — none are used anywhere in any .rs file.
D5. Redundant dev-dependencies in qql-plan
crates/qql-plan/Cargo.toml:24-25 — qql-core and serde_json duplicated in [dev-dependencies] with identical specs to [dependencies]. Cargo makes regular deps available to tests automatically.
D6. serde "derive" feature unused in qql-edge
crates/qql-edge/Cargo.toml:39 — No type in qql-edge derives Serialize/Deserialize. The "derive" feature can be dropped.
D7. serde direct dep unused in pyqql
crates/pyqql/Cargo.toml:22 — serde is a direct dependency but never used directly; all serialization goes through serde_json and pythonize.
D8. Empty impl CollectionSchema {} block
crates/qql-runtime/src/backend.rs:197 — Zero methods. Pure dead code.
D9. parse_optional_vector_name() marked #[allow(dead_code)]
crates/qql-core/src/parser/helpers.rs:27-28 — Never wired up.
🟡 ARCHITECTURE & CONTRACT ISSUES
A1. Qdrant API specifics leak into qql-core (violates crate boundary)
AGENT.md line 56 says qql-core "Has NO knowledge of Qdrant endpoints, REST JSON shapes, or transport protocols." But the following files contain OpenAPI/proto/gRPC/HTTP details:
- src/ast/statement.rs:230-234 — "Read consistency for Qdrant point reads... OpenAPI... proto... REST... gRPC..."
- src/ast/statement.rs:260-263 — "OpenAPI query param / proto field" for timeout
- src/ast/statement.rs:237-246 — ReadConsistency enum (Qdrant transport concept)
- src/parser/with_clause.rs:28,30,250 — OpenAPI ReadConsistency details
- src/error.rs:35,37 — Transport/backend error descriptions
A2. SearchParams mixes search parameters with request metadata
crates/qql-core/src/ast/statement.rs:248-265 — SearchParams contains real search params (hnsw_ef, exact, quantization, rrf_k, acorn) AND transport metadata (timeout, consistency). These are different concerns — transport fields should live in qql-runtime.
A3. FilterCompound.shard_key field always None
crates/qql-plan/src/filter.rs — Every construction site passes shard_key: None. Either the field is dead or the feature is incomplete.
A4. QueryRequest.lookup_from and CountRequest.exact always hardcoded None
crates/qql-plan/src/query.rs:399, src/plan.rs:346 — Fields exist in the types but are never populated, representing incomplete coverage of the Qdrant API.
A5. shard_key() method on PlannedOperation only covers 6/22 variants
crates/qql-plan/src/plan.rs:191-202 — UpdatePayload, ClearPayload, UpdateVectors, DeleteVectors, CreateShardKey, DropShardKey all silently return None for shard key info even though their request types carry shard key fields.
A6. OpenAPI used without version constraint
crates/qql-runtime/src/contract_test.rs — Contract tests validate against whatever openapi.json is checked in, with no version tracking. If it's updated to a newer Qdrant version, tests may pass against incompatible server schemas.
A7. execute_cross_rerank dedup loses data silently
crates/qql-runtime/src/executor/mod.rs:948-958 — Uses HashMap by hit.id to deduplicate across collections. If the same point ID exists in multiple collections, only the first one is kept, silently losing the others.
A8. extract_search_hits has fragile multi-path response parsing
crates/qql-runtime/src/executor/dml/query.rs:54-91 — Tries 3 fallback paths (gRPC-style, REST-style, batch-style) without validating which is correct for the current response.
A9. RequestBody enum missing variants for body-less operations
crates/qql-plan/src/routing.rs:8-26 — DropIndex, ListShardKeys, etc. produce body: None and rely on method+path matching in downstream route_statement_type. This caused bugs B2 and B3 above.
📄 DOCUMENTATION ISSUES
DOC1. docs/syntax.md:12-14 — statement EBNF production is incomplete
Missing drop-index, drop-shard-key, show-shard-keys, etc. in the top-level statement production.
DOC2. qql-plan/README.md:31 — variant count says "21" but PlannedOperation has 22 (missing CrossRerank)
DOC3. qql-plan/src/ddl.rs:381 — stale comment contradicting code
Comment says "shard_keys is not part of CreateCollection" but the IR struct has shard_keys: Option<Vec<String>> and it IS populated.
DOC4. qql-core/src/parser/mod.rs:29-31 — stale doc claiming pest grammar is used
Says "Syntax acceptance is generated from language/v1/grammar.pest" but actual parsing uses the manual AstLowerer, not the pest grammar.
DOC5. qql-plan/Cargo.toml README claims serde/std features that don't exist in Cargo.toml
DOC6. CHANGELOG.md [Unreleased] section is incomplete
Missing CrossRerank, CLEAR PAYLOAD, DELETE VECTOR, SHOW SHARD KEYS, DropIndex entries.
DOC7. qql-edge README says list_embedding_models lists "dense text" models but actually also lists image models
DOC8. pyqql-edge test expects __version__ export that doesn't exist
tests/test_comprehensive.py:36 — Test always fails.
🔧 FASTEMBED INTEGRATION vs fastembed-rs.md
Feature	Status	Notes
Text (dense) embedding	✅ Full support	30+ models via TextEmbedding
Sparse embedding (Splade/BGE-M3)	❌ Not available	Only local BM25; trait embed_sparse has no model param
Image embedding (CLIP/ResNet/Unicom)	✅ Opt-in	ImageEmbedding via image-models feature
Reranking	✅ Opt-in	TextRerank via edge/host config
BGE-M3 joint (dense+sparse+ColBERT)	⚠️ Separate calls	No single-pass joint embed in trait
Qwen3 / Nomic v2 MoE	❌ Not available	Candle-backend, not in ONNX Embedder trait
CLIP text encoder as dense model	✅ Works	Through generic dense embedding path
📊 SUMMARY
Category	Count
🔴 Bugs (incorrect behavior)	8
🟡 Half-baked / incomplete	8
🟢 Dead code / cleanup	9
🟡 Architecture / contract	9
📄 Documentation	8
🔧 Fastembed gaps	4
Top 5 things to fix first:
1. B1 — gRPC silently drops datatype on vector creation
2. B2/B3 — DropIndex/ListShardKeys compile() returns wrong stmt_type
3. B7 — Auto-embedding always adds both dense+sparse regardless of topology
4. B4 — CROSS_RERANK field extraction fallback silently uses wrong field
5. A1 — Qdrant API specifics leaking into qql-core (violates crate boundary contract)