import type {
	AnalysisResult,
	CompiledRoute,
	ExecutionReport,
} from "qql-wasm-current";

export type EmbedProvider = "browser" | "http" | "none";
export type PolicyValueType = "string" | "number" | "boolean";
export type InspectorTab =
	| "plan"
	| "wire"
	| "ast"
	| "tokens"
	| "explain"
	| "response"
	| "metrics";
export type ExportLanguage = "python" | "node" | "rust" | "curl";

export interface PlaygroundSettings {
	qdrantUrl: string;
	qdrantKey: string;
	embedProvider: EmbedProvider;
	embedUrl: string;
	embedModel: string;
	embedDim: number;
	embedKey: string;
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

/** Structured execution failure: `buildError`-style parsing of the WASM throw. */
export interface PlaygroundFailure {
	/** Human sentence shown on the card and in the toast. */
	message: string;
	/** Machine code (`QQL-MISSING-USING`, …) — null when unparseable. */
	code: string | null;
	kind: string | null;
	span: { start: number; end: number } | null;
	fields: Record<string, string>;
	/** Raw throw text for the collapsible details. */
	raw: string;
}

/** Live collection vector topology for USING suggestions and presets. */
export interface LiveCollectionTopology {
	name: string;
	dense: string[];
	sparse: string[];
	multi: string[];
}

export interface PlaygroundAnalysis {
	source: string;
	result: AnalysisResult;
	effectiveAst: unknown[] | null;
	effectiveRoutes: CompiledRoute[];
	policyError: string | null;
}

export interface PlaygroundState {
	analysis: PlaygroundAnalysis | null;
	response: ExecutionReport | null;
	executionError: PlaygroundFailure | null;
	selectedStatement: number;
	inspectorTab: InspectorTab;
	exportLanguage: ExportLanguage;
	metrics: PlaygroundMetrics | null;
}

/** Documented live environment: LM Studio OpenAI-compatible embeddings. */
export const LIVE_EMBED_URL = "http://localhost:1234/v1/embeddings";
export const LIVE_EMBED_MODEL = "text-embedding-bge-small-en-v1.5";
export const LIVE_EMBED_DIM = 384;

/** Pre-LM-Studio defaults; stored settings matching these migrate forward. */
const STALE_EMBED_URL = "http://localhost:11434/v1/embeddings";
const STALE_EMBED_MODEL = "nomic-embed-text";
const STALE_EMBED_DIM = 768;

export const DEFAULT_SETTINGS: PlaygroundSettings = {
	qdrantUrl: "http://localhost:6333",
	qdrantKey: "",
	embedProvider: "http",
	embedUrl: LIVE_EMBED_URL,
	embedModel: LIVE_EMBED_MODEL,
	embedDim: LIVE_EMBED_DIM,
	embedKey: "",
};

/** Rewrite stale Ollama-era embedder settings to the LM Studio endpoint. */
export function migrateSettings(settings: PlaygroundSettings): boolean {
	if (
		settings.embedUrl === STALE_EMBED_URL &&
		settings.embedModel === STALE_EMBED_MODEL &&
		settings.embedDim === STALE_EMBED_DIM
	) {
		settings.embedUrl = LIVE_EMBED_URL;
		settings.embedModel = LIVE_EMBED_MODEL;
		settings.embedDim = LIVE_EMBED_DIM;
		// Untouched installs still point at the old browser default provider;
		// the documented live environment is the LM Studio HTTP endpoint.
		if (settings.embedProvider === "browser") {
			settings.embedProvider = "http";
		}
		return true;
	}
	return false;
}

export const DEFAULT_POLICY: RuntimePolicy = {
	enabled: false,
	field: "tenant_id",
	op: "=",
	value: "acme",
	valueType: "string",
	shardKey: "acme",
};
