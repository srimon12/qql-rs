/** QQL landing copy. Keep it short: the comparator does the convincing. */

export const hero = {
	headline: "SQL for Qdrant.",
	lede: "QQL is to Qdrant what SQL is to Postgres. One query for hybrid search, filters, and schema.",
	eyebrow: "Open source · MIT · v0.3.1",
	specimenTitle: "search.qql",
	specimenMeta: "QQL · MIT",
	primaryCta: { label: "Try the playground", href: "/playground/" },
	secondaryCta: {
		label: "Quickstart",
		href: "/docs/getting-started/quickstart/",
	},
} as const;

export const stats = [
	{ value: "12", label: "Query forms" },
	{ value: "276", label: "Conformance statements" },
	{ value: "6", label: "Runtimes & tools" },
	{ value: "1.4 MB", label: "WASM parser, in-browser" },
] as const;

export const problem = {
	heading: "Same query. No boilerplate.",
	sub: "Pick REST JSON or any SDK client. The QQL stays a few lines.",
	guideHref: "/docs/guides/qql-vs-qdrant-json/",
	guideLabel: "Full comparison in the docs",
} as const;

export const pipeline = {
	heading: "How a statement runs.",
	sub: "One pass from source to dispatch. The plan is the contract between every runtime.",
	steps: [
		{
			step: "01",
			title: "Parse",
			detail:
				"qql-core lexes and parses into a typed AST. Malformed clauses fail with a span, never a silent default.",
		},
		{
			step: "02",
			title: "Validate",
			detail:
				"Named vectors resolve against the collection schema. Unknown USING kinds fail closed.",
		},
		{
			step: "03",
			title: "Plan",
			detail:
				"plan() lowers the AST to one transport-neutral PlannedOperation.",
		},
		{
			step: "04",
			title: "Dispatch",
			detail:
				"The same plan projects to REST JSON, gRPC protobuf, or the in-process edge backend.",
		},
	],
} as const;

export const language = {
	heading: "One grammar. The whole surface.",
	sub: "Hybrid retrieval, faceting, formula scoring, recommendations — twelve query forms over one typed grammar.",
	footHref: "/docs/language/",
	footLabel: "Language reference",
} as const;

export const getStarted = {
	heading: "Install",
	sub: "CLI, Python, Node, Rust, WASM, and VS Code.",
	installs: [
		{
			name: "CLI",
			cmd: "curl -fsSL https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.sh | sh",
			href: "/docs/getting-started/installation/",
		},
		{ name: "Python", cmd: "pip install pyqql", href: "/docs/sdks/python/" },
		{ name: "Node.js", cmd: "npm i @veristamp/nqql", href: "/docs/sdks/node/" },
		{ name: "Rust", cmd: "cargo add qql qql-core", href: "/docs/sdks/rust/" },
		{ name: "WASM", cmd: "npm i qql-wasm", href: "/docs/sdks/wasm/" },
		{ name: "VS Code", cmd: "srimon12.qql-lang", href: "/docs/tools/editors/" },
	],
	footHref: "/docs/getting-started/quickstart/",
	footLabel: "Quickstart",
} as const;

export const cta = {
	heading: "One query. Every runtime.",
	body: "The playground runs the real WASM parser in your browser — no cluster, no signup.",
	primaryCta: { label: "Try the playground", href: "/playground/" },
	secondaryCta: { label: "Read the docs", href: "/docs/" },
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
				"Rust crates, native Python and Node.js bindings, a ~1.4 MB WASM package, the qql CLI, and a VS Code extension with live diagnostics.",
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
				"It is young: v0.3.1. Fail-closed defaults, OpenAPI contract tests, a conformance corpus, and a public gaps document. The API surface is stabilizing, not frozen.",
		},
		{
			question: "Can I try it without a cluster?",
			answer:
				"Yes. The playground runs the real WASM parser in the browser. qql-edge runs the pipeline with local HNSW storage and ONNX embeddings.",
		},
	],
} as const;
