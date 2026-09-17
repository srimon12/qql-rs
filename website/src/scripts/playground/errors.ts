import type { PlaygroundFailure } from "../playground-types";

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
				return `The embedding endpoint failed (${parsed.code}). Check the endpoint, model, and dimension.`;
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

interface ParsedQqlError {
	code: string;
	kind?: string;
	message: string;
	span: { start: number; end: number } | null;
	fields: Record<string, string>;
}

/** `buildError` port: WASM throws JSON-serialized QqlError strings, not Errors. */
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

/** Structured failure with a human sentence; keeps the parsed `.code`. */
export function buildFailure(
	error: unknown,
	qdrantUrl: string,
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
				fields: {},
				raw,
			};
		}
		return {
			message: raw || "The query failed with an unknown error.",
			code: null,
			kind: null,
			span: null,
			fields: {},
			raw,
		};
	}
	return {
		message: humanizeFailure(parsed, host),
		code: parsed.code,
		kind: parsed.kind ?? null,
		span: parsed.span,
		fields: parsed.fields,
		raw,
	};
}
