/** QQL landing page copy — mirrors Veristamp's apps/landing/src/data/landing.ts. */

export const hero = {
  badge: "MIT licensed by Veristamp",
  headline: "A query language for Qdrant.",
  tagline: "QQL is to Qdrant what SQL is to Postgres.",
  lede: "One typed language for search, hybrid retrieval, filtering, mutations, and schema. It parses, plans, and executes across Rust, Python, Node.js, WASM, REST, gRPC, and edge.",
  primaryCta: { label: "Read the quickstart", href: "/docs/getting-started/quickstart/" },
  secondaryCta: { label: "Try the playground", href: "/playground/" },
  hosts: ["Rust", "Python", "Node.js", "WASM", "CLI", "VS Code"],
  terminalTitle: "quickstart.qql",
} as const;

export const stats = [
  { value: "19", label: "Statement types" },
  { value: "12", label: "Query expressions" },
  { value: "4", label: "Language bindings" },
  { value: "3", label: "Backends" },
] as const;

export const problem = {
  label: "The problem",
  heading: "Every request, hand-assembled.",
  sub: "Filter JSON built per call, embeddings invoked by hand, tenant predicates repeated in every code path. QQL replaces that surface with one typed language.",
  rows: [
    {
      without: "Nested filter JSON, manual embedding calls, one client per SDK",
      with: "One SQL-like surface for search, hybrid, mutations, and DDL",
    },
    {
      without: "Tenant filters pasted into every code path: miss one and data leaks",
      with: "inject_filter rewrites the AST before planning. Recursive, fail closed",
    },
    {
      without: "Application code tied to REST, or gRPC, or an edge store",
      with: "Plan once, then execute over REST, gRPC, or in-process edge unchanged",
    },
  ],
} as const;

export const language = {
  label: "The language",
  heading: "Reads like SQL. Plans like Qdrant.",
  sub: "Hybrid fusion, multi-stage CTEs, shard routing, and formula scoring are part of the grammar, not string templates.",
  footHref: "/docs/language/",
  footLabel: "Full language reference →",
} as const;

export const architecture = {
  label: "Architecture",
  heading: "Parse, plan, execute: separate crates.",
  sub: "Parsing never knows about endpoints. Planning never does I/O. Execution never re-interprets the language.",
  steps: [
    {
      step: "01",
      title: "Parse",
      detail: "Hand-written lexer and typed AST. Stable error codes, byte-accurate spans.",
    },
    {
      step: "02",
      title: "Prepare",
      detail: "Schema-aware vector kinds, dense / sparse / multi embeddings, upsert preparation.",
    },
    {
      step: "03",
      title: "Plan",
      detail: "One transport-neutral PlannedOperation IR. No JSON-as-IR.",
    },
    {
      step: "04",
      title: "Execute",
      detail: "REST, gRPC, or edge: same language, same response envelope.",
    },
  ],
} as const;

export const design = {
  label: "Design",
  heading: "Built to be inspected.",
  sub: "The same parser, planner, and error codes in every runtime. Fix a bug once and every binding gets it.",
  capabilities: [
    {
      eyebrow: "Language",
      title: "The full Qdrant surface, one grammar",
      body: "Nearest, recommend, discover, hybrid RRF, formula scoring, MMR, cross-encoder rerank, mutations, indexes, and shard keys: a 670-line canonical grammar with 163 keywords.",
    },
    {
      eyebrow: "Security",
      title: "Tenant isolation at the AST",
      body: "Hosts inject trusted predicates into every CTE and prefetch before planning. Isolation is the security boundary; SHARD routing stays a separate locality concern.",
    },
    {
      eyebrow: "Architecture",
      title: "Parse, plan, execute",
      body: "qql-core is no_std with zero dependencies. One transport-neutral PlannedOperation IR projects to REST, raw tonic gRPC, or qdrant-edge.",
    },
    {
      eyebrow: "Edge",
      title: "The full pipeline, in-process",
      body: "Local HNSW storage with ONNX embeddings. No server, no ports, no API keys. Unsupported cluster features fail loudly with stable error codes.",
    },
    {
      eyebrow: "Tooling",
      title: "CLI, IDE, formatter",
      body: "qql exec, explain, doctor, and fmt. A VS Code extension with live diagnostics from the real WASM parser, and an agent skill so LLMs write correct QQL.",
    },
    {
      eyebrow: "Quality",
      title: "The grammar is the contract",
      body: "Conformance fixtures, OpenAPI contract tests against Qdrant's schema, and every documented example parsed at build time. The docs cannot drift from the parser.",
    },
  ],
} as const;

export const limits = {
  label: "Current limits",
  statement: "Young, Qdrant-specific, and honest about it.",
  body: "v0.2.1 is weeks old and the API surface is stabilizing, not frozen. Edge is intentionally single-node: group-by, custom shard routing, and replication are rejected with stable error codes. Benchmarks are single-run samples, not a CI suite. The gaps document is public.",
  foot: "Fail-closed by default, stable error codes, public gaps doc",
} as const;

export const getStarted = {
  label: "Get started",
  heading: "Install it where you already work.",
  sub: "crates.io, PyPI, npm, VS Code Marketplace, and GitHub Releases.",
  installs: [
    { name: "CLI", cmd: "curl -fsSL …/install.sh | sh", href: "/docs/getting-started/installation/" },
    { name: "Python", cmd: "pip install pyqql", href: "/docs/sdks/python/" },
    { name: "Node.js", cmd: "npm i @veristamp/nqql", href: "/docs/sdks/node/" },
    { name: "Rust", cmd: "cargo add qql qql-core", href: "/docs/sdks/rust/" },
    { name: "WASM", cmd: "npm i qql-wasm", href: "/docs/sdks/wasm/" },
    { name: "VS Code", cmd: "srimon12.qql-lang", href: "/docs/tools/editors/" },
  ],
  primaryCta: { label: "Read the quickstart", href: "/docs/getting-started/quickstart/" },
  secondaryCta: { label: "Run offline on edge", href: "/docs/edge/getting-started/" },
} as const;

export const faq = {
  label: "FAQ",
  heading: "Common questions",
  items: [
    {
      question: "What is QQL?",
      answer:
        "A typed, declarative query language for Qdrant. One surface covers retrieval, filtering, mutations, schema operations, and policy-safe AST rewriting.",
    },
    {
      question: "Which runtimes ship today?",
      answer:
        "Rust crates, native Python and Node.js bindings (plus edge variants), a ~1.3 MB WebAssembly package, the qql CLI, and a VS Code extension with live WASM diagnostics.",
    },
    {
      question: "How does multitenancy work?",
      answer:
        "Parse untrusted QQL, then inject a trusted tenant filter into the AST before planning. Custom SHARD routing is a separate locality concern and can run alongside the filter.",
    },
    {
      question: "Does QQL replace Qdrant?",
      answer:
        "No. QQL plans operations for Qdrant and dispatches them over REST or gRPC, or evaluates the supported subset through the in-process edge backend.",
    },
    {
      question: "Is it production-ready?",
      answer:
        "It is young: v0.2.1. Fail-closed defaults, OpenAPI contract tests, a conformance corpus, and a public gaps document. The API surface is stabilizing, not frozen.",
    },
    {
      question: "Can I try it without a cluster?",
      answer:
        "Yes. The playground runs the real WASM parser in-browser, and qql-edge runs the full pipeline with local HNSW storage and ONNX embeddings (offline after models cache).",
    },
  ],
} as const;

export const cta = {
  label: "Start here",
  heading: "Read the quickstart. Run a query in five minutes.",
  body: "Parse, enforce policy, plan, and execute: from a laptop edge process to a remote Qdrant cluster.",
  primaryCta: { label: "Read the quickstart", href: "/docs/getting-started/quickstart/" },
  secondaryCta: { label: "Try the playground", href: "/playground/" },
} as const;
