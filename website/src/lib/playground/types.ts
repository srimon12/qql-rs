import type {
	AnalysisResult,
	CompiledRoute,
	ExecutionReport,
} from "qql-wasm-current";

export type EmbedProvider = "browser" | "http" | "none";
export type PolicyValueType = "string" | "number" | "boolean";
export type InspectorTab =
	| "response"
	| "plan"
	| "wire"
	| "ast"
	| "tokens"
	| "explain"
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

export interface PlaygroundFailure {
	message: string;
	code: string | null;
	kind: string | null;
	span: { start: number; end: number } | null;
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

export interface PlaygroundState {
	analysis: PlaygroundAnalysis | null;
	response: ExecutionReport | null;
	executionError: PlaygroundFailure | null;
	selectedStatement: number;
	inspectorTab: InspectorTab;
	exportLanguage: ExportLanguage;
	metrics: PlaygroundMetrics | null;
}

export const LIVE_EMBED_URL = "http://localhost:1234/v1/embeddings";
export const LIVE_EMBED_MODEL = "text-embedding-bge-small-en-v1.5";
export const LIVE_EMBED_DIM = 384;
export const DEFAULT_QDRANT_URL = "http://localhost:6333";

export const DEFAULT_SETTINGS: PlaygroundSettings = {
	qdrantUrl: DEFAULT_QDRANT_URL,
	qdrantKey: "",
	embedProvider: "http",
	embedUrl: LIVE_EMBED_URL,
	embedModel: LIVE_EMBED_MODEL,
	embedDim: LIVE_EMBED_DIM,
	embedKey: "",
};

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
