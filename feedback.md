Comprehensive Codebase Analysis: qql-rs
I've read through every significant file across all 6 workspace crates: qql-core, qql-runtime (published as qql), qql-cli, qql-wasm, nqql, pyqql.
Here's the full diagnosis:
🏛️ GRAMMAR / UX — SQL-Native Feel
The language is good but has several SQL-inconsistencies that hurt learnability:
1. WITH ... AS is parsed but not PostgreSQL-style
- WITH name AS (...) uses the QQL's with_clause (search params like HNSW ef), conflicting with standard SQL CTE syntax. A user who knows SQL will write WITH cte AS (QUERY ...) SELECT ... and hit confusion.
- Fix: Use WITH ... FOR SEARCH for QQL-specific search directives, reserve bare WITH ... AS for CTEs. Or use OPTIONS(...) like other databases.
2. QUERY feels awkward as a top-level keyword
- Standard SQL doesn't have QUERY x FROM y. Instead: SELECT ... FROM y ORDER BY vector_distance(...). 
- Consider: SELECT * FROM collection NEAREST '<text>' reads like SQL extended with a clause, rather than a separate QUERY statement type.
- SHOW COLLECTIONS / SHOW COLLECTION x is good — very MySQL-like.
3. Underscore-heavy compound tokens break the SQL flow
- GROUP_SIZE, SCORE_THRESHOLD (single tokens GROUP_SIZE / SCORE_THRESHOLD) make the lexer treat them as monolithic keywords, but SQL users expect GROUP BY x GROUP SIZE 10 or SCORE THRESHOLD 0.5 as two tokens.
- GEO_BBOX, GEO_RADIUS, VALUES_COUNT, HAS_VECTOR — same issue.
- The lexer (line 196-209) already handles [] subscript paths but the compound keyword approach is fragile.
4. UPDATE for vectors vs payload is ambiguous  
- UPDATE collection SET PAYLOAD = ... and UPDATE collection SET VECTOR = ... look similar but route to completely different Qdrant APIs (set_payload vs update_vectors). 
- SQL's UPDATE ... SET x = 1, y = 2 can't set both in one statement. Real SQL would accept UPDATE collection SET payload.field = value, vector = [...].
5. ALTER COLLECTION does not match SQL DDL semantics
- In SQL, ALTER TABLE allows multiple operations: ALTER TABLE t ADD COLUMN, DROP COLUMN, RENAME. Here it's just ALTER COLLECTION x WITH ... which is really UPDATE COLLECTION CONFIG.
- If you're going full SQL-native: ALTER COLLECTION x SET HNSW m = 16.
6. No USE / CONNECT statement for switching collections / databases
🚨 ARCHITECTURAL ISSUES
Critical: QqlError in qql-core is no_std — but holds Cow<'static, str>
// qql-core/src/error.rs
pub struct QqlError {
    pub msg: Cow<'static, str>,  // Cow<'static> means you can only use static strings or owned Strings
    pub pos: usize,
}
Problem: The syntax() constructor takes impl Into<Cow<'static, str>>, but the only way to get a Cow::Owned is with String (which is 'static due to ownership). However, format!() produces a String → Cow::Owned(String) which works, but you're coupling the no_std crate to alloc::format! vs core::format!. This works in practice but is semantically misleading — if this is truly no_std, the std::error::Error impl at line 37 should be behind a #[cfg(feature = "std")] gate. Currently it always compiles impl std::error::Error for QqlError {} — a hard std dependency from what claims to be no_std.
Critical: qql-wasm crate renames the runtime crate to q
// qql-wasm/Cargo.toml
q = { package = "qql", path = "../qql-runtime", ... }
But in qql-core, the runtime crate is imported as qql everywhere else. This is confusing and fragile — someone reading q::offline::compile() has to remember the alias.
QdrantOps trait couples transport logic with business logic
The QdrantOps trait (16 methods) mixes:
- Collection management (DDL)
- Point CRUD (DML)
- Search (query variants, groups, batch)
- Index management
For a first-class Qdrant library this is fine. But it makes unit testing harder — every mock must stub all 16 methods. Consider splitting into sub-traits: CollectionOps, PointOps, SearchOps, IndexOps. Then:
pub trait QdrantOps: CollectionOps + PointOps + SearchOps + IndexOps {}
Lifetime pollution throughout AST
The entire AST in qql-core is parameterized with <'a> — every struct, every enum, every Value<'a> variant. This means:
- You can't easily store parsed ASTs (they borrow from the input string)
- The to_static() trampoline on Value is necessary but expensive
- The executor constantly calls stmt.collection.map(|s| s.to_string()) to break free from lifetimes → clones everywhere
- The SDK wrappers (nqql, pyqql, wasm) all parse → serialize → deserialize to get owned values
Better approach: either use Cow<'a, str> pervasively (already done in many places), or parse into an owned AST from the start using String fields. The current half-borrowed/half-owned pattern is the worst of both worlds.
⚡ PERFORMANCE BOTTLENECKS
1. serde_json::to_value → serde_json::from_value roundtrips everywhere
In executor/dml/query.rs:
let payload_map: Option<HashMap<String, serde_json::Value>> = hit.payload
    .as_ref()
    .and_then(|p| serde_json::from_value(serde_json::to_value(p).unwrap()).ok());
This serializes p to a Value then immediately deserializes it to HashMap. If p is already a HashMap<String, Value>, just clone it. This pattern appears in 6+ places across executor/dml/query.rs and executor/mod.rs.
2. serde_json::to_value on the full AST during parsing (SDKs)
In nqql/src/lib.rs, pyqql/src/lib.rs, qql-wasm/src/lib.rs:
serde_json::to_value(&stmt)  // serializes the entire AST to JSON
Every SDK call to parse() serializes the AST to JSON just to pass it to the host language. If the host only needs to execute, this is pure overhead. Consider a dedicated execute() path that skips JSON serialization entirely.
3. remove_nulls on every query serialization (rest.rs)
fn remove_nulls(value: &mut Value) {
    // recursive retainer
}
Called on every query_request_json() and grouped_query_request_json(). This does a full recursive walk of the JSON just to strip nulls. Better to fix the serializer to skip None fields (use #[serde(skip_serializing_if = "Option::is_none")] on fields instead).
4. tokio::time::sleep(500ms) after every collection creation
// ddl.rs line 249-250
#[cfg(any(feature = "rest", feature = "grpc"))]
tokio::time::sleep(std::time::Duration::from_millis(500)).await;
Hardcoded 500ms delay in the executor, not in the transport layer. This blocks the thread for every CREATE COLLECTION call. The ?wait=true query param already waits for completion — this 500ms is a magic constant that should not exist in library code.
5. rest.rs query_batch sequentially queries
async fn query_batch(...) -> Result<...> {
    let mut results = Vec::with_capacity(req.len());
    for query in req {  // sequential!
        results.push(self.query(query).await?);
    }
    Ok(results)
}
The REST backend doesn't batch — it loops. The gRPC backend may differ but the trait accepts Vec<QueryPointsRequest> for a reason. The REST impl should use futures::join_all or similar. Qdrant's REST API actually supports batch query at /collections/{name}/points/query/batch.
🔄 CROSS-SDK CODE DUPLICATION
The serde_json_to_value converter is copy-pasted in 3 SDKs:
- nqql/src/lib.rs:120-161 (41 lines)
- qql-wasm/src/lib.rs:83-124 (41 lines)
- pyqql/src/lib.rs:292-326 uses a different pattern (py_to_value)
All three convert serde_json::Value → qql_core::ast::Value. This should live in qql-core as a public conversion function.
The create_js_executor / create_executor pattern is duplicated:
- nqql/src/lib.rs:206-287 (81 lines)
- pyqql/src/lib.rs:152-212 (60 lines)
Both parse the same options dict to build QqlConfig, create REST/gRPC client, create HttpEmbedder, and wire them into Executor. This is the exact same logic. It should live in qql-runtime as a builder or factory method.
🦀 RUST-SPECIFIC CODE QUALITY ISSUES
1. Unnecessary pub(crate) visibility leaking
- executor/dml/helpers.rs:extract_point_id is pub(crate) but only used inside dml/insert.rs. Should be private.
- executor/dml/config.rs exists as a module with no content (empty directory?)
2. unwrap() in non-test code
- executor/mod.rs:462: serde_json::from_value(serde_json::to_value(p).unwrap()).ok() — that .unwrap() will panic on serialization failure.
- rest.rs:257: body.as_object_mut().expect("...") — expect in production transport code.
- rest.rs:100: envelope.get("result").cloned().unwrap_or(envelope) uses envelope after a partial move (works only because Value implements Clone).
3. The with_clause / SearchWith struct has mmr_diversity / mmr_candidates at the top level
pub struct SearchWith {
    pub hnsw_ef: u64,
    pub exact: bool,
    pub acorn: bool,
    // ...
    pub mmr_diversity: Option<f64>,   // MMR is a sub-feature of SEARCH
    pub mmr_candidates: Option<u64>,  // same
    pub rrf_k: Option<u64>,           // RRF is a fusion method
    pub rrf_weights: Vec<f32>,        // same
}
These mix orthogonal concerns. mmr_* is a diversification post-processing, rrf_* is a fusion technique. A cleaner design:
pub struct SearchWith {
    pub hnsw_ef: u64,
    pub exact: bool,
    pub acorn: bool,
    pub quantization: Option<QuantizationSearchWith>,
    pub fusion: Option<FusionConfig>,  // RRF config
    pub mmr: Option<MmrConfig>,
}
4. FormulaExpr has both Case and MatchCondition which duplicate filter logic
- FormulaExpr::Case { cond: FilterExpr, ... } embeds a full FilterExpr inside a formula. This creates a recursive dependency between the formula and filter systems without clear semantics.
- FormulaExpr::MatchCondition { field, values } replicates FilterExpr::In/FilterExpr::MatchAny logic.
5. qql-core/Cargo.toml — phf dependency with macros feature for just the keyword map
- The KEYWORDS map uses phf::phf_map!. This adds compile-time overhead for what could be a simple match or hashbrown table. For a no_std core crate, this is a fair choice, but phf is somewhat niche.
📦 PACKAGING / PORTABILITY ISSUES
1. WASM crate depends on qql-runtime via the q alias — but never uses the runtime
- qql-wasm/src/lib.rs uses qql_core::ast and qql_core::parser but imports q only for q::offline::compile and q::executor::Executor::explain. Both of these only depend on qql-core + serde_json.
- The runtime feature flag is required for explain() but explain() itself is purely local (no network calls). Consider moving explain() into qql-core or offline.
2. qql-cli/Cargo.toml depends on qql-runtime with default features
- This means both grpc and rest features are always pulled in. The crate should match its actual needs.
3. pyqql/pyproject.toml and nqql/package.json are untracked in this review, but the Cargo.toml files suggest Python/Node.js build dependencies (maturin, napi) are managed separately — ensure they pin compatible versions.
🎯 HIGHEST-PRIORITY FIXES
#	Issue	Impact	File(s)
1	std::error::Error in no_std crate	Blocks embedded/WASM without std	core/src/error.rs:37
2	500ms sleep in create_collection	Blocks all collection creation	runtime/src/executor/ddl.rs:249
3	serde_json roundtrip for payload conversion	CPU waste on every query result	executor/dml/query.rs:404-407
4	remove_nulls on every REST query	CPU waste per request	runtime/src/rest.rs:121-136
5	Duplicated serde_json_to_value across 3 SDKs	Maintenance burden	nqql, pyqql, wasm lib.rs
6	REST query_batch is sequential	Missed optimization	runtime/src/rest.rs:327-336
7	Lifetime parametric AST (<'a>)	Complicates ownership, causes clones	Every AST struct in qql-core
8	QdrantOps monolithic trait	Hard to mock/test	runtime/src/client.rs:115
9	SDK executor construction duplicated	150+ lines copy-pasted	nqql, pyqql lib.rs
10	Compound token keywords (GROUP_SIZE, GEO_BBOX, etc.)	Hurts SQL-native feel	core/src/token.rs
🧪 WHAT'S DONE WELL (not just criticism)
- Pipeline architecture is excellent. ExecutionNode trait + QueryPipeline + QueryState is clean, extensible, and testable.
- backend.rs separation (transport-neutral types) is solid — it's what would make LanceDB integration plausible.
- FilterConverter is clean and well-organized.
- offline::compile() is a smart API — parse without a server.
- Error messages from FilterConverter are informative.
- The codebase is well-commented with useful //! module docs and inline explanations.
- dml/config.rs as a module directory suggests you're actively organizing — finish filling it.