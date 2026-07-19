Review complete
I reviewed all workspace crates, docs, CI, parser/runtime paths, CLI, and FFI bindings. I also updated AGENT.md with corrected architecture, safety invariants, CLI truthfulness, grammar rules, and verification commands.
Highest-priority improvements
1. Fix CLI truthfulness or implement the missing behavior
- qql exec only explains; it does not execute.
- connect claims success without connecting.
- dump writes a mock empty export.
- The README claims all three work against Qdrant.
- Plan: either rename them to plan, repl, and dump-template, or first add a real QdrantOps HTTP/gRPC adapter and wire configuration into the CLI.
2. Make grammar validation strict
- Invalid input is accepted:
- QUERY 'x' FROM docs LIMIT -1
- SCROLL FROM docs LIMIT -1
- QUERY CONTEXT PAIRS
- Negative limits are later cast to u64, producing huge request values.
- Several parser branches return Ok(()) after malformed clauses, silently retaining defaults.
- Plan: introduce validated newtypes such as Limit(NonZeroU64) / Offset(u64), return QqlError for every malformed clause, and add regression tests.
3. Replace the script splitter
- qql-cli/src/script.rs infers boundaries from statement keywords instead of semicolons.
- It breaks top-level WITH ... QUERY ... statements and accepts missing delimiters unexpectedly.
- Plan: split only on lexer-recognized top-level semicolons, or parse the whole script through Parser::parse_all.
4. Correct documentation and binding contracts
- CLI README shows syntax the parser does not accept, including tuple-style INSERT.
- Runtime README refers to QdrantOperations, but the actual trait is QdrantOps.
- Python, Node, and WASM READMEs claim string/debug returns or JS names that do not match exported functions.
- Plan: make tested examples the documentation source of truth; add README example tests.
Rust stability improvements
5. Remove silent data corruption paths
- Runtime/offline code commonly turns serialization failures into null, empty data, or ignored .ok() values.
- PointId conversion can fabricate Num(0) when an external ID is malformed.
- value_to_json converts non-finite floats into 0.0.
- Generated-type conversions contain production unwrap() calls.
- Plan: propagate typed QqlErrors with context; use TryFrom, never fabricated fallback values.
6. Harden transport and configuration
- HttpEmbedder has no explicit request timeout or bounded error-body read.
- Config secrets are stored as plaintext JSON under ~/.qql.
- Collection creation relies on existence checks plus fixed 500ms sleeps, which are race-prone.
- Plan: apply configured timeouts, protect config file permissions or use environment/keyring options, and poll operation readiness.
7. Stabilize the Qdrant boundary
- The runtime defines an abstraction and generated API types, but no production QdrantOps implementation exists—only mocks.
- build.rs mutates the OpenAPI schema before generation, making generated-shape changes sensitive.
- Plan: ship and integration-test one real adapter, pin/schema-test generated conversions, and test against a Qdrant compatibility matrix.
Quality roadmap
- Phase 1 — correctness: strict numeric validation, script splitter, no silent conversion fallbacks, CLI/documentation honesty.
- Phase 2 — real UX: real Qdrant adapter, configured CLI, structured JSON errors, confirmation for destructive actions.
- Phase 3 — release stability: MSRV/toolchain pinning, CI clippy/check, CLI integration tests, Python/Node/WASM smoke tests, Qdrant compatibility tests.
Verification run
Passed locally:
cargo fmt --all -- --check
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --workspace --all-targets
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo check --workspace --all-targets
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo clippy --workspace --all-targets -- -D warnings
Rust unit tests pass: 341 total. The main gap is that CLI and foreign bindings currently have effectively no behavioral integration coverage.