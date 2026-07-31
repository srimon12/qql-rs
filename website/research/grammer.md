Complete Investigation Report
1. The Problem: Four Hand-Maintained Keyword Lists, All Drifting from grammar.pest
Location	Lines	Content	Status
language/v1/grammar.pest	~140+ ^"..." tokens	Source of truth	✅ Authoritative
editors/vscode/syntaxes/qql.tmLanguage.json:69-95	6 categorized regex groups	Syntax highlighting keywords	❌ Drifted
editors/vscode/src/completions.ts:5-44	~85 keywords in flat array	Autocompletion	❌ Drifted (includes BOOST, STAR, AVERAGE_VECTOR, ID — some not in grammar)
website/src/scripts/playground-language.ts:9-53	~50 keywords + 3 values	CodeMirror highlighting/completion	❌ Drifted (includes SELECT, TOP, FULL, CLUSTER — pre-v1 aliases)
crates/qql-core/src/token.rs:287-389	102 keywords in phf_map	Runtime token lookup	Also hand-maintained, must match grammar
2. Design: Extend qql-grammar-gen to Derive All Keyword Artifacts from grammar.pest
The existing crate already does: grammar.pest → crates/qql-core/grammar/qql.generated.pest with generate/check commands. Extend it to additionally produce all downstream keyword lists.
2a. Keyword Extraction Strategy (from grammar.pest)
In pest, ^"TOKEN" means a case-insensitive literal match.  Regex target: \^\s*"([^"]+)" distinguishing ^"KEYWORD" from bare "(", ")" etc.
Three classes emerge from the grammar:
- UPPERCASE_KEYWORDS (most ^"QUERY", ^"FROM", etc.) — the core statement/query/ddl vocabulary  
- lowercase_keywords (^"average_vector", ^"hnsw_ef", ^"datetime", ^"true", etc.) — parameter names, function names, enum values  
- Constants (^"true", ^"false", ^"null") — semantic booleans/null, not syntactic keywords
2b. Intermediate Manifest Output
New generated file: language/v1/keywords.json
{
  "version": "1.3",
  "generated_by": "qql-grammar-gen",
  "source": "language/v1/grammar.pest",
  "generated_at": "ISO-8601",
  "keywords": [
    "ABS", "ACORN", "AFTER", "ALTER", "AND", "ANY", "AS", "ASC",
    "BETWEEN", "BY", "CANDIDATES", "CASE", "CLEAR", "COLLECTION",
    "COLLECTIONS", "CONTEXT", "COSINE", "COUNT", "CREATE", "CROSS",
    "DBSF", "DEFAULTS", "DELETE", "DENSE", "DESC", "DISCOVER",
    "DIVERSITY", "DOT", "DROP", "ELSE", "EMBED", "EMPTY", "END",
    "EUCLID", "EXACT", "EXP", "EXP_DECAY", "FEEDBACK", "FIELD",
    "FOR", "FORMULA", "FROM", "FUSION", "GAUSS_DECAY", "GEO_BBOX",
    "GEO_DISTANCE", "GEO_POLYGON", "GEO_RADIUS", "GROUP",
    "HAS_VECTOR", "HNSW", "HYBRID", "IMAGE", "IN", "INDEX",
    "INTO", "IS", "KEY", "KEYS", "LIMIT", "LIN_DECAY", "LN",
    "LOG", "LOOKUP", "MANHATTAN", "MATCH", "MATCH_ANY", "MMR",
    "MODEL", "MULTI", "MULTIVECTOR", "NAIVE", "NEAREST",
    "NEGATIVE", "NESTED", "NOT", "OFFSET", "ON", "OPTIMIZERS",
    "OR", "ORDER", "PARAMS", "PAYLOAD", "PHRASE", "POINT",
    "POINTS", "POSITIVE", "POW", "PREFETCH", "QUANTIZATION",
    "QUERY", "RANDOM", "RECOMMEND", "RELEVANCE", "RERANK",
    "RRF", "SAMPLE", "SCORE", "SCROLL", "SET", "SHARD", "SHOW",
    "SIZE", "SPARSE", "SQRT", "STRATEGY", "TARGET", "TEXT",
    "THEN", "THRESHOLD", "TYPE", "UPDATE", "UPSERT", "USING",
    "VALUES", "VALUES_COUNT", "VECTOR", "WHEN", "WHERE", "WITH",
    // ... field types, distance kinds, remaining config keys
  ],
  "lowercase_keywords": [
    "a", "acorn", "all", "average_vector", "b", "best_score",
    "bool", "bottom_right", "c", "center", "consistency",
    "datetime", "datetime_key", "decay", "e", "exact",
    "exterior", "float", "geo", "hnsw_ef", "id", "ignore",
    "indices", "indexed_only", "integer", "interiors",
    "keyword", "lat", "lon", "majority", "max_selectivity",
    "midpoint", "oversampling", "quorum", "quantization",
    "radius", "rescore", "rrf_k", "rrf_weights", "scale",
    "sum_scores", "target", "text", "timeout", "top_left",
    "uuid", "values", "x",
  ],
  "constants": ["false", "null", "true"],
  "groups": {
    "control": ["ALTER", "CLEAR", "COUNT", "CREATE", "DELETE",
                 "DROP", "EMBED", "QUERY", "SCROLL", "SHOW",
                 "UPDATE", "UPSERT"],
    "other": ["AFTER", "AS", "BY", "FIELD", "FOR", "FROM", "INTO",
              "LIMIT", "MODEL", "OFFSET", "ON", "PARAMS", "SCORE",
              "SHARD", "THRESHOLD", "TYPE", "USING", "VALUES",
              "WHERE", "WITH", ...],
    "query": ["CANDIDATES", "CONTEXT", "CROSS", "DBSF", "DENSE",
              "DISCOVER", "DIVERSITY", "FUSION", "HYBRID",
              "IMAGE", "MMR", "MULTI", "MULTIVECTOR", "NAIVE",
              "NEAREST", "NEGATIVE", "PAYLOAD", "POINT", "POINTS",
              "POSITIVE", "PREFETCH", "RANDOM", "RECOMMEND",
              "RELEVANCE", "RERANK", "RRF", "SAMPLE", "SPARSE",
              "STRATEGY", "TARGET", "TEXT", "VECTOR", ...],
    "ddl": ["ACORN", "BINARY", "BITS", "COLLECTION", "COLLECTIONS",
            "CONSISTENCY", "COSINE", "DISABLED", "DOT", "EUCLID",
            "EXACT", "HNSW", "INDEX", "KEY", "KEYS", "MAJORITY",
            "MANHATTAN", "OPTIMIZERS", "PARAMS", "PRODUCT",
            "QUANTILE", "QUANTIZATION", "QUORUM", "RAM",
            "SCALAR", "TIMEOUT", "TURBO", ...],
    "filter": ["ANY", "BETWEEN", "EMPTY", "GEO_BBOX", "GEO_POLYGON",
               "GEO_RADIUS", "HAS_VECTOR", "IN", "IS", "MATCH",
               "NESTED", "NOT", "NULL", "OR", "PHRASE",
               "VALUES_COUNT", ...],
    "formula": ["ABS", "CASE", "DEFAULTS", "ELSE", "END", "EXP",
                "EXP_DECAY", "GAUSS_DECAY", "GEO_DISTANCE",
                "LIN_DECAY", "LN", "LOG", "POW", "SQRT",
                "THEN", "WHEN", ...],
  }
}
The groups field maps keywords to a TextMate category based on which grammar rules contain them. This derivation is:
- Keywords in statement children (query, scroll, count, etc.) → control
- Keywords in query_tail → other
- Keywords in query_expr children (nearest, hybrid, recommend, etc.) → query
- Keywords in DDL rules (create_collection, alter_collection, etc.) → ddl
- Keywords in predicate_tail → filter
- Keywords in formula rules → formula
- true/false/null → constant (separate from keywords)
2c. Target Artifact Generation
All three consumer files are regenerated by the same generate command:
Output	Path	Content Change
Existing: crates/qql-core/grammar/qql.generated.pest	No change	Already generated
New: language/v1/keywords.json	Full	Intermediate manifest
New: editors/vscode/syntaxes/qql.tmLanguage.json	Keyword sections only	6 groups filled from keywords.json; all other sections (comments, strings, numbers, operators, punctuation, identifiers) remain as-is in the Rust template
New: editors/vscode/src/completions.ts	Lines 5-44 only	KEYWORDS array regenerated from deduped keyword list
New: website/src/scripts/playground-language.ts	Lines 9-53 only	keywords Set and values Set regenerated
3. Integration Points
qql-grammar-gen (Rust crate)
File: crates/qql-grammar-gen/src/main.rs
Extend: Add new subcommands or make generate/check multi-output:
cargo run -p qql-grammar-gen -- generate
  → crates/qql-core/grammar/qql.generated.pest        (existing)
  → language/v1/keywords.json                          (new)
  → editors/vscode/syntaxes/qql.tmLanguage.json        (new)
  → editors/vscode/src/completions.ts                   (new, keyword section)
  → website/src/scripts/playground-language.ts          (new)

cargo run -p qql-grammar-gen -- check
  → verifies ALL five outputs are current
Add a new dependency: serde + serde_json for JSON manifest output. The tmLanguage JSON can be built with serde_json::json!() macro or serde::Serialize structs. The TypeScript file is string-templated.
language/v1/README.md
Update the generation section (line 37-49) to mention the new outputs. The existing cargo run -p qql-grammar-gen -- generate command still works — it just produces more files.
CI (ci.yml)
Line 231: Already runs cargo run --locked -p qql-grammar-gen -- check — no change needed. The check command will now also verify the new artifacts.
Line 230: The conformance job name doesn't need to change.
Release (release.yml)
Line 54: Already runs cargo run --locked -p qql-grammar-gen -- check — no change needed.
Website deploy (deploy-site.yml)
No change needed — all keyword artifacts are pre-committed and verified by CI before they reach deploy.
VS Code build (editors/vscode/scripts/build.mjs)
No change needed — syntaxes/qql.tmLanguage.json and src/completions.ts are pre-committed and only change when generate is run.
Website package.json
No change needed — playground-language.ts is pre-committed.
4. Verification Strategy
Check	When	What verifies
cargo run -p qql-grammar-gen -- check	CI conformance job + local dev	All 5 generated outputs match grammar.pest
pnpm validate:qql	Website check script	Documentation QQL examples parse correctly via WASM (separate concern)
cargo test -p qql-core	CI rust job	Tokenizer uses KEYWORDS phf_map, AST lowering uses correct tokens — if keyword generation reveals discrepancies, tests break
Conformance runner	CI conformance job	Fixtures parse end-to-end
If a PR changes grammar.pest but doesn't run generate, the check step in CI fails with a clear message ("X is stale; run cargo run -p qql-grammar-gen -- generate").
5. Risks
Risk	Impact	Mitigation
Categorization accuracy — the heuristic (which grammar rule a keyword appears in) may assign some keywords to the wrong TextMate category, causing highlight regressions	Medium	First generate run produces a diff that can be manually reviewed. Add a small number of overrides in Rust code for ambiguous cases.
False positives — regex \^\s*"([^"]+)" also matches some non-keyword tokens (e.g. comparison operators, "(", ")")	Low	Filter by character set ([A-Za-z_] only); operators use `{ "!="
Lowercase keyword handling — tokens like ^"average_vector", ^"hnsw_ef" are case-insensitive in pest but don't follow the usual UPPERCASE keyword convention	Medium	Separate them into a lowercase_keywords list; the playground and completions may or may not want them. Existing consumers currently only have uppercase.
Stale keywords removed — STAR, BOOST, SELECT, TOP, FULL, CLUSTER exist in hand-maintained lists but not in grammar.pest	Low (correctness)	This is the desired outcome — stale keywords die. Document in the PR/changelog.
First-generation diff size — the initial generate run replaces all keyword lists, likely producing a large diff	Low	One-time review cost. After that, diffs are small (only when grammar changes).
VS Code tmLanguage formatting — current file has hand-optimized regex patterns; auto-generated patterns may use different whitespace/ordering	Low	Generate with consistent formatting; the check comparison is byte-exact.
6. Implementation Sketch
Suggested structure for crates/qql-grammar-gen/src/:
src/
  main.rs              ← CLI dispatch (existing, extend)
  grammar.rs           ← pest parsing, rule tree traversal (new)
  keywords.rs          ← extraction + categorization logic (new)
  render.rs            ← output rendering (new)
    pest_render()      → qql.generated.pest (existing render.rs code, moved)
    keywords_render()  → keywords.json
    tm_language_render() → qql.tmLanguage.json
    completions_render() → completions.ts (keyword section)
    playground_render()  → playground-language.ts
  check.rs             ← verification (existing, extend)
The Rust template for tmLanguage.json preserves the non-keyword sections (comments, strings, numbers, operators, punctuation, identifiers) verbatim as a const string or serde_json::Value, injecting only the 6 keyword match patterns.
