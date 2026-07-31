# QQL documentation architecture

The website documents the current `qql-rs` workspace. It does not preserve the
retired `qql-go` CLI, Go SDK, or Connect gateway surface.

## Source-of-truth order

1. `language/v1/fixtures` — executable syntax examples and canonical ASTs.
2. `language/v1/grammar.pest` and `language/v1/spec` — grammar and semantics.
3. `qql-core::Parser::parse_all` — production full-script parser.
4. Current crate APIs and generated SDK declarations.
5. Prose in `website/src/content/docs/docs`.

When these disagree, fix the prose or implementation contract instead of
inventing a documentation-only dialect.

## Information architecture

| Section | Reader question | Pages |
|---|---|---|
| Start | What is QQL and how do I run it? | overview, installation, quickstart, execution model |
| Language | What can I write? | queries, filters, data operations, collections/indexes, formulas, scripts/errors |
| Guides | How do I solve production retrieval problems? | hybrid retrieval, embeddings/reranking, multitenancy, backends |
| SDKs | How do I integrate QQL in my host? | Rust, Python, Node.js, WebAssembly |
| Tools | How do I operate or learn it? | CLI, examples |
| Reference | What is the exact contract? | grammar, API surface, backend compatibility |
| Contributing | How do I change docs safely? | development, executable-doc verification |

## Content rules

- Put complete, runnable QQL in `{% qqlExample %}`. The docs verifier extracts
  every instance and parses it with a freshly built Node-target WASM SDK.
- Use `terminal` for shell commands, `glassCard` for conceptual choices, and
  `apiField` for API members. Do not simulate these components with ad-hoc HTML.
- Keep execution claims transport-neutral unless a REST, gRPC, or edge
  difference is the point of the page.
- Keep isolation (`WHERE` / `inject_filter`) separate from physical routing
  (`SHARD` / `shard_key`).
- Generate playground presets directly from every valid v1 fixture. Do not
  duplicate fixture source inside the website.
- Link runnable `qqlExample` blocks into `/playground/?q=…` so documentation
  and interactive exploration share the exact same query.
- Link release history to GitHub Releases. Do not duplicate invented changelog
  entries inside the documentation tree.

## Integrated playground

The playground is an Astro route in the same application. Astro components own
the accessible shell, shared site chrome, and fixture rendering. A single
browser controller owns CodeMirror, qql-wasm initialization, explicit
`Client`/`Stmt` cleanup, analysis, policy injection, and execution.

The website build generates the browser package from the current
`crates/qql-wasm` source. TypeScript resolves its declaration file separately
from Vite's runtime JavaScript alias so generated `.d.ts` and `.js` identities
do not drift.
