import type { Diagnostic } from "@codemirror/lint";
import initQql, {
	analyze,
	type CompiledRoute,
	formatQuery,
	Stmt,
} from "qql-wasm-current";
import type {
	EmbedProvider,
	PlaygroundAnalysis,
	PlaygroundFailure,
	PlaygroundMetrics,
	RuntimePolicy,
} from "./types";

export { formatQuery, initQql };

export function byteOffsetToPosition(source: string, offset: number): number {
	const bytes = new TextEncoder().encode(source);
	const safeOffset = Math.max(0, Math.min(offset, bytes.length));
	return new TextDecoder().decode(bytes.slice(0, safeOffset)).length;
}

export function policyValue(rule: RuntimePolicy): string | number | boolean {
	if (rule.valueType === "number") {
		const num = Number(rule.value);
		if (!Number.isFinite(num))
			throw new Error("Policy value must be a number.");
		return num;
	}
	if (rule.valueType === "boolean") {
		if (rule.value !== "true" && rule.value !== "false") {
			throw new Error("Boolean policy values must be true or false.");
		}
		return rule.value === "true";
	}
	return rule.value;
}

export function applyRuntimePolicy(
	statement: Stmt,
	policy: RuntimePolicy,
): void {
	statement.injectFilter(policy.field, policy.op, policyValue(policy));
	if (policy.shardKey.trim()) statement.shardKey = policy.shardKey.trim();
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

/**
 * Inline marker for the span a failed run reported (executor errors carry
 * byte offsets into the run's source). Empty when the failure has no span.
 */
export function runtimeDiagnostic(
	failure: PlaygroundFailure | null,
	source: string,
): Diagnostic[] {
	if (!failure?.span) return [];
	if (!source) return [];
	const offset = failure.startOffset ?? 0;
	const localSource = source.slice(offset);
	const from = Math.min(
		offset + byteOffsetToPosition(localSource, failure.span.start),
		source.length,
	);
	const rawTo = Math.min(
		offset + byteOffsetToPosition(localSource, failure.span.end),
		source.length,
	);
	const to = Math.max(from + 1, rawTo);
	return [
		{
			from,
			to: Math.min(to, source.length),
			severity: "error",
			message: `${failure.code ?? "QQL-ERROR"}: ${failure.message}`,
		},
	];
}

export function formatError(error: unknown): string {
	if (error instanceof Error) return error.message;
	if (typeof error === "string") return error;
	try {
		return JSON.stringify(error);
	} catch {
		return String(error);
	}
}

export function connectionHost(url: string): string {
	try {
		const parsed = new URL(url);
		return parsed.host || url;
	} catch {
		return url.replace(/^https?:\/\//, "");
	}
}

interface ParsedQqlError {
	code: string;
	kind?: string;
	message: string;
	span: { start: number; end: number } | null;
	fields: Record<string, string>;
}

export function parseQqlError(raw: string): ParsedQqlError | null {
	let parsed: unknown;
	try {
		parsed = JSON.parse(raw);
	} catch {
		return null;
	}
	if (!parsed || typeof parsed !== "object") return null;
	const record = parsed as Record<string, unknown>;
	if (typeof record.code !== "string") return null;
	const fields: Record<string, string> = {};
	const rawFields = record.fields;
	if (Array.isArray(rawFields)) {
		for (const field of rawFields) {
			if (field && typeof field === "object") {
				const entry = field as Record<string, unknown>;
				if (typeof entry.key === "string" && entry.value != null) {
					fields[entry.key] = String(entry.value);
				}
			}
		}
	}
	const span = record.span as { start: number; end: number } | null | undefined;
	return {
		code: record.code,
		kind: typeof record.kind === "string" ? record.kind : undefined,
		message: typeof record.message === "string" ? record.message : raw,
		span:
			span && typeof span.start === "number" && typeof span.end === "number"
				? { start: span.start, end: span.end }
				: null,
		fields,
	};
}

function humanizeFailure(
	parsed: { code: string; message: string; fields: Record<string, string> },
	host: string,
): string {
	switch (parsed.code) {
		case "QQL-MISSING-USING":
			return "This text query needs a USING clause to pick a vector.";
		case "QQL-BACKEND-COLLECTION-NOT-FOUND": {
			const name =
				parsed.fields.collection ??
				/Collection `([^`]+)`/.exec(parsed.message)?.[1] ??
				/Collection '([^']+)'/.exec(parsed.message)?.[1] ??
				"this collection";
			return `Collection '${name}' doesn't exist on this Qdrant.`;
		}
		case "QQL-BACKEND-DIMENSION-MISMATCH": {
			const dims = /expected dim: (\d+), got (\d+)/.exec(parsed.message);
			return dims
				? `Vector size doesn't match the collection (needs ${dims[1]}, got ${dims[2]}).`
				: "Vector size doesn't match the collection.";
		}
		case "QQL-TRANSPORT":
		case "QQL-TRANSPORT-CONNECT":
		case "QQL-TRANSPORT-TIMEOUT":
			return `Qdrant unreachable at ${host} — check connection`;
		default: {
			if (parsed.code.startsWith("QQL-TRANSPORT")) {
				return `Qdrant unreachable at ${host} — check connection`;
			}
			if (parsed.code.startsWith("QQL-EMBEDDING")) {
				return `The embedding endpoint failed (${parsed.code}). Check endpoint, model, and dimension.`;
			}
			if (parsed.code.startsWith("QQL-BACKEND")) {
				const status = parsed.fields.status_code;
				return status
					? `Qdrant rejected the request (HTTP ${status}).`
					: "Qdrant rejected the request.";
			}
			return parsed.message;
		}
	}
}

export function buildFailure(
	error: unknown,
	qdrantUrl: string,
	startOffset = 0,
): PlaygroundFailure {
	const raw = formatError(error);
	const host = connectionHost(qdrantUrl);
	const parsed = parseQqlError(raw);
	if (!parsed) {
		if (
			error instanceof TypeError ||
			/failed to fetch|networkerror|load failed|network request failed|connection refused/i.test(
				raw,
			)
		) {
			return {
				message: `Qdrant unreachable at ${host} — check connection`,
				code: "QQL-TRANSPORT",
				kind: "Transport",
				span: null,
				startOffset,
				fields: {},
				raw,
			};
		}
		return {
			message: raw || "The query failed with an unknown error.",
			code: null,
			kind: null,
			span: null,
			startOffset,
			fields: {},
			raw,
		};
	}
	return {
		message: humanizeFailure(parsed, host),
		code: parsed.code,
		kind: parsed.kind ?? null,
		span: parsed.span,
		startOffset,
		fields: parsed.fields,
		raw,
	};
}
