import type { Diagnostic } from "@codemirror/lint";
import { analyze, type CompiledRoute, Stmt } from "qql-wasm-current";
import { applyRuntimePolicy } from "./client";
import { formatError } from "./errors";
import type {
	EmbedProvider,
	PlaygroundAnalysis,
	PlaygroundMetrics,
	RuntimePolicy,
} from "../playground-types";

export function byteOffsetToPosition(source: string, offset: number): number {
	const bytes = new TextEncoder().encode(source);
	const safeOffset = Math.max(0, Math.min(offset, bytes.length));
	return new TextDecoder().decode(bytes.slice(0, safeOffset)).length;
}

export function analyzeWithPolicy(
	source: string,
	policy: RuntimePolicy,
	embedProvider: EmbedProvider,
	prevExecuteMs: number | null,
): { analysis: PlaygroundAnalysis; metrics: PlaygroundMetrics } {
	const started = performance.now();
	const result = analyze(source);
	let effectiveAst = result.ast;
	let effectiveRoutes = result.routes;
	let policyError: string | null = null;

	if (result.valid && policy.enabled) {
		if (result.statements_count !== 1) {
			policyError =
				"Runtime policy injection accepts exactly one statement. Multi-statement scripts fail closed.";
		} else {
			let statement: Stmt | null = null;
			try {
				statement = new Stmt(source);
				applyRuntimePolicy(statement, policy);
				effectiveAst = [statement.toObject()];
				effectiveRoutes = [statement.compileRoute()];
			} catch (error) {
				policyError = formatError(error);
			} finally {
				statement?.free();
			}
		}
	}

	const metrics: PlaygroundMetrics = {
		analyzedAt: new Date().toISOString(),
		parseMs: performance.now() - started,
		executeMs: prevExecuteMs,
		statements: result.statements_count,
		policyApplied: policy.enabled && !policyError && result.valid,
		embedProvider,
	};
	return {
		analysis: { source, result, effectiveAst, effectiveRoutes, policyError },
		metrics,
	};
}

/** Synthetic fallback so a WASM-side analyze throw still renders diagnostics. */
export function fallbackAnalysis(
	source: string,
	message: string,
): PlaygroundAnalysis {
	// The synthetic fallback must satisfy the current AnalysisResult shape:
	// `errors` is the primary diagnostic list; `error` is kept for older clients.
	const diagnostic = {
		code: "QQL-WASM",
		message,
		start: null,
		end: null,
	};
	return {
		source,
		result: {
			valid: false,
			statements_count: 0,
			tokens: [],
			ast: null,
			route: null,
			routes: [],
			explain: null,
			error: diagnostic,
			errors: [diagnostic],
		},
		effectiveAst: null,
		effectiveRoutes: [],
		policyError: null,
	};
}

export function currentDiagnostic(
	analysis: PlaygroundAnalysis | null,
): Diagnostic[] {
	if (!analysis) return [];
	const { source, result, policyError } = analysis;
	if (policyError) {
		const firstLine = source.indexOf("\n");
		return [
			{
				from: 0,
				to: Math.max(
					1,
					firstLine === -1 ? Math.min(source.length, 1) : firstLine,
				),
				severity: "error",
				message: policyError,
			},
		];
	}
	if (result.valid) return [];
	const extra = (result as { errors?: NonNullable<typeof result.error>[] })
		.errors;
	const analysisErrors =
		extra && extra.length > 0 ? extra : result.error ? [result.error] : [];
	if (analysisErrors.length === 0) return [];
	return analysisErrors.map((err) => {
		const from =
			err.start == null ? 0 : byteOffsetToPosition(source, err.start);
		const rawTo =
			err.end == null ? from + 1 : byteOffsetToPosition(source, err.end);
		return {
			from: Math.min(from, source.length),
			to: Math.min(Math.max(from + 1, rawTo), source.length),
			severity: "error" as const,
			message: `${err.code}: ${err.message}`,
		};
	});
}

export function selectedRoute(
	analysis: PlaygroundAnalysis | null,
	selected: number,
): CompiledRoute | null {
	return analysis?.effectiveRoutes[selected] ?? analysis?.result.route ?? null;
}
