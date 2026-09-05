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
	executionError: string | null;
	selectedStatement: number;
	inspectorTab: InspectorTab;
	exportLanguage: ExportLanguage;
	metrics: PlaygroundMetrics | null;
}

export const DEFAULT_SETTINGS: PlaygroundSettings = {
	qdrantUrl: "http://localhost:6333",
	qdrantKey: "",
	embedProvider: "browser",
	embedUrl: "http://localhost:11434/v1/embeddings",
	embedModel: "nomic-embed-text",
	embedDim: 768,
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
