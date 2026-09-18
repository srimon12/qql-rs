import type {
	AnalysisResult,
	CompiledRoute,
	ExecutionReport,
} from "qql-wasm-current";

export type EmbedProvider = "browser" | "http" | "none";
export type BrowserEmbedDevice = "auto" | "webgpu" | "wasm";
export type PolicyValueType = "string" | "number" | "boolean";
export type MobileView = "editor" | "result";
export type StatementStatus = "idle" | "running" | "ok" | "error" | "skipped";
export type InspectorTab =
	| "response"
	| "plan"
	| "wire"
	| "ast"
	| "tokens"
	| "explain"
	| "metrics";
export type ExportLanguage = "python" | "node" | "rust" | "curl";

/** Char offsets of one statement in the editor document (`;` included). */
export interface StatementSpan {
	/** First char after the previous `;` — leading comments/whitespace included. */
	start: number;
	/** Just past the terminating `;`, or the end of the document. */
	end: number;
}

export interface PlaygroundSettings {
	qdrantUrl: string;
	qdrantKey: string;
	embedProvider: EmbedProvider;
	embedUrl: string;
	embedModel: string;
	embedDim: number;
	embedKey: string;
	/** Transformers.js model id used by the in-browser embedder. */
	embedBrowserModel: string;
	/** Preferred inference backend; `auto` probes WebGPU then falls back. */
	embedBrowserDevice: BrowserEmbedDevice;
}

/** Run controls for multi-statement scripts. */
export interface RunPreferences {
	/** `false` maps to the executor's `onError: "continue"` batch mode. */
	stopOnError: boolean;
}

export interface RuntimePolicy {
	enabled: boolean;
	field: string;
	op: string;
	value: string;
	valueType: PolicyValueType;
	shardKey: string;
}

export interface PlaygroundMetrics {
	analyzedAt: string;
	parseMs: number;
	executeMs: number | null;
	statements: number;
	policyApplied: boolean;
	embedProvider: EmbedProvider;
}

export interface PlaygroundFailure {
	message: string;
	code: string | null;
	kind: string | null;
	span: { start: number; end: number } | null;
	startOffset?: number;
	fields: Record<string, string>;
	raw: string;
}

export interface PlaygroundAnalysis {
	source: string;
	result: AnalysisResult;
	effectiveAst: unknown[] | null;
	effectiveRoutes: CompiledRoute[];
	policyError: string | null;
}

/**
 * Every piece of playground state in one place. Persisted fields are
 * rehydrated by `store.ts` from localStorage; the rest is per-session.
 */
export interface PlaygroundState {
	// ── Persisted ──
	settings: PlaygroundSettings;
	policy: RuntimePolicy;
	runPrefs: RunPreferences;
	inspectorTab: InspectorTab;
	exportLanguage: ExportLanguage;
	mobileView: MobileView;
	wrapEnabled: boolean;

	// ── Session ──
	analysis: PlaygroundAnalysis | null;
	metrics: PlaygroundMetrics | null;
	selectedStatement: number;
	response: ExecutionReport | null;
	executionError: PlaygroundFailure | null;
	executeMs: number | null;
	/** Text-level statement map of the live document. */
	statementSpans: StatementSpan[];
	/** Editor statement indices covered by the last run, in execution order. */
	executedStatements: number[];
	statementStatus: Map<number, StatementStatus>;
	/** Real tensor width reported by the in-browser model, once loaded. */
	browserDims: number | null;
}

export const LIVE_EMBED_URL = "http://localhost:1234/v1/embeddings";
export const LIVE_EMBED_MODEL = "text-embedding-bge-small-en-v1.5";
export const LIVE_EMBED_DIM = 384;
export const DEFAULT_QDRANT_URL = "http://localhost:6333";

/** Curated browser models. `dims` is display metadata only — the pipeline
 * still reports the real tensor width after the first embedding. */
export const BROWSER_MODELS: Array<{
	id: string;
	label: string;
	dims: number;
	note: string;
}> = [
	{
		id: "Xenova/all-MiniLM-L6-v2",
		label: "MiniLM L6 v2",
		dims: 384,
		note: "Smallest download, English",
	},
	{
		id: "Xenova/bge-small-en-v1.5",
		label: "BGE small EN v1.5",
		dims: 384,
		note: "Sharper English retrieval",
	},
	{
		id: "Xenova/bge-base-en-v1.5",
		label: "BGE base EN v1.5",
		dims: 768,
		note: "Larger, better recall",
	},
	{
		id: "Xenova/gte-small",
		label: "GTE small",
		dims: 384,
		note: "Balanced general text",
	},
	{
		id: "Xenova/multilingual-e5-small",
		label: "Multilingual E5 small",
		dims: 384,
		note: "100+ languages",
	},
	{
		id: "Xenova/paraphrase-multilingual-MiniLM-L12-v2",
		label: "Paraphrase multilingual MiniLM",
		dims: 384,
		note: "Translation-tolerant matching",
	},
];

export const DEFAULT_BROWSER_MODEL = BROWSER_MODELS[0].id;

export function browserModelInfo(id: string): {
	label: string;
	dims: number | null;
} {
	const known = BROWSER_MODELS.find((model) => model.id === id);
	if (known) return { label: known.label, dims: known.dims };
	const short = id.split("/").at(-1) ?? id;
	return { label: short || "Browser model", dims: null };
}

export const DEFAULT_SETTINGS: PlaygroundSettings = {
	qdrantUrl: DEFAULT_QDRANT_URL,
	qdrantKey: "",
	embedProvider: "browser",
	embedUrl: LIVE_EMBED_URL,
	embedModel: LIVE_EMBED_MODEL,
	embedDim: LIVE_EMBED_DIM,
	embedKey: "",
	embedBrowserModel: DEFAULT_BROWSER_MODEL,
	embedBrowserDevice: "auto",
};

export const DEFAULT_RUN_PREFERENCES: RunPreferences = { stopOnError: true };

export const DEFAULT_POLICY: RuntimePolicy = {
	enabled: false,
	field: "tenant_id",
	op: "=",
	value: "acme",
	valueType: "string",
	shardKey: "acme",
};

export const SETTINGS_KEY = "qql-playground.settings.v1";
export const POLICY_KEY = "qql-playground.policy.v2";
export const WORKSPACE_KEY = "qql-playground.workspace.v1";
export const INSPECTOR_TAB_KEY = "qql-playground.inspector-tab.v1";
export const SPLIT_KEY = "qql-playground.split.v1";
export const RUN_KEY = "qql-playground.run.v1";
export const WRAP_KEY = "qql-playground.wrap.v1";
export const MOBILE_VIEW_KEY = "qql-playground.mobile-view.v1";
