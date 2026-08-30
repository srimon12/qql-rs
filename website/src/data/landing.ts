/** QQL landing copy. Keep it short: the comparator does the convincing. */

export const hero = {
  headline: "SQL for Qdrant.",
  lede: "QQL is to Qdrant what SQL is to Postgres. One query for hybrid search, filters, and schema.",
  primaryCta: { label: "Try the playground", href: "/playground/" },
  secondaryCta: { label: "Quickstart", href: "/docs/getting-started/quickstart/" },
} as const;

export const problem = {
  heading: "Same query. Less JSON.",
  sub: "Switch REST JSON or the Python client. The QQL stays a few lines.",
  guideHref: "/docs/guides/qql-vs-qdrant-json/",
  guideLabel: "Full comparison in the docs",
} as const;

export const why = {
  heading: "What you stop writing.",
  rows: [
    {
      label: "Filter trees",
      body: "A WHERE clause instead of nested must/should JSON in every call.",
    },
    {
      label: "Tenant copies",
      body: "inject_filter rewrites the AST before planning. Miss a path and it fails closed.",
    },
    {
      label: "Three clients",
      body: "Plan once. REST, gRPC, and in-process edge share the same operation.",
    },
  ],
} as const;

export const getStarted = {
  heading: "Install",
  sub: "CLI, Python, Node, Rust, WASM, and VS Code.",
  installs: [
    { name: "CLI", cmd: "curl -fsSL …/install.sh | sh", href: "/docs/getting-started/installation/" },
    { name: "Python", cmd: "pip install pyqql", href: "/docs/sdks/python/" },
    { name: "Node.js", cmd: "npm i @veristamp/nqql", href: "/docs/sdks/node/" },
    { name: "Rust", cmd: "cargo add qql qql-core", href: "/docs/sdks/rust/" },
    { name: "WASM", cmd: "npm i qql-wasm", href: "/docs/sdks/wasm/" },
    { name: "VS Code", cmd: "srimon12.qql-lang", href: "/docs/tools/editors/" },
  ],
  footHref: "/docs/getting-started/quickstart/",
  footLabel: "Quickstart",
} as const;

export const faq = {
  heading: "Questions",
  items: [
    {
      question: "What is QQL?",
      answer:
        "A typed query language for Qdrant. One surface for retrieval, filtering, mutations, schema, and policy-safe AST rewriting.",
    },
    {
      question: "Which runtimes ship today?",
      answer:
        "Rust crates, native Python and Node.js bindings, a ~1.3 MB WASM package, the qql CLI, and a VS Code extension with live diagnostics.",
    },
    {
      question: "How does multitenancy work?",
      answer:
        "Parse untrusted QQL, then inject a trusted tenant filter into the AST before planning. SHARD routing is a separate locality concern and can run alongside the filter.",
    },
    {
      question: "Does QQL replace Qdrant?",
      answer:
        "No. QQL plans operations for Qdrant and dispatches them over REST or gRPC, or evaluates the supported subset through the in-process edge backend.",
    },
    {
      question: "Is it production-ready?",
      answer:
        "It is young: v0.3.0. Fail-closed defaults, OpenAPI contract tests, a conformance corpus, and a public gaps document. The API surface is stabilizing, not frozen.",
    },
    {
      question: "Can I try it without a cluster?",
      answer:
        "Yes. The playground runs the real WASM parser in the browser. qql-edge runs the pipeline with local HNSW storage and ONNX embeddings.",
    },
  ],
} as const;
