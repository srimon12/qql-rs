import { Client, type ExecutionReport, Stmt } from "qql-wasm-current";
import { createBrowserEmbedder } from "../browser-embedder";
import type {
	LiveCollectionTopology,
	PlaygroundSettings,
	RuntimePolicy,
} from "../playground-types";

export function applyRuntimePolicy(
	statement: Stmt,
	policy: RuntimePolicy,
): void {
	statement.injectFilter(policy.field, policy.op, policyValue(policy));
	if (policy.shardKey.trim()) statement.shardKey = policy.shardKey.trim();
}

export function policyValue(rule: RuntimePolicy): string | number | boolean {
	if (rule.valueType === "number") {
		const number = Number(rule.value);
		if (!Number.isFinite(number))
			throw new Error("Policy value must be a number.");
		return number;
	}
	if (rule.valueType === "boolean") {
		if (rule.value !== "true" && rule.value !== "false") {
			throw new Error("Boolean policy values must be true or false.");
		}
		return rule.value === "true";
	}
	return rule.value;
}

/**
 * WASM client lifecycle: one live client, retired predecessors freed once no
 * execution is in flight. Collapses the old `retiredClients` + `activeExecutions`
 * globals into the only place that touches them.
 */
export class QdrantClient {
	private current: Client | null = null;
	private retired = new Set<Client>();
	private active = 0;

	get ready(): boolean {
		return this.current !== null;
	}

	/**
	 * Rebuild the client from settings. Returns the embedder status note for
	 * the connection footer. Throws when HTTP embeddings are misconfigured.
	 */
	configure(
		settings: PlaygroundSettings,
		onBrowserEmbedStatus: (message: string) => void,
	): string {
		const previous = this.current;
		const next = new Client(settings.qdrantUrl, settings.qdrantKey || null);

		let note: string;
		if (settings.embedProvider === "browser") {
			next.setEmbedder(createBrowserEmbedder(onBrowserEmbedStatus));
			note = "Browser model loads only when execution needs text embeddings.";
		} else if (settings.embedProvider === "http") {
			if (!settings.embedUrl || !settings.embedModel || settings.embedDim < 1) {
				next.free();
				throw new Error(
					"HTTP embeddings need an endpoint, model, and dimension.",
				);
			}
			next.setHttpEmbedder(
				settings.embedUrl,
				settings.embedModel,
				settings.embedDim,
				settings.embedKey || null,
			);
			note = `Using ${settings.embedModel} through ${settings.embedUrl}.`;
		} else {
			note = "Text embedding is disabled; use explicit vectors.";
		}

		this.current = next;
		if (previous) {
			if (this.active === 0) previous.free();
			else this.retired.add(previous);
		}
		return note;
	}

	/** Parse → policy-inject → embed → POST. Throws the raw WASM value. */
	async execute(
		source: string,
		policy: RuntimePolicy,
	): Promise<ExecutionReport> {
		const client = this.current;
		if (!client) throw new Error("Qdrant client is not configured.");
		let statement: Stmt | null = null;
		this.active += 1;
		try {
			if (policy.enabled) {
				statement = new Stmt(source);
				applyRuntimePolicy(statement, policy);
				return await client.executeStmt(statement);
			}
			return await client.execute(source);
		} finally {
			statement?.free();
			this.active -= 1;
			if (this.active === 0) {
				for (const retired of this.retired) retired.free();
				this.retired.clear();
			}
		}
	}

	free(): void {
		this.current?.free();
		this.current = null;
		for (const retired of this.retired) retired.free();
		this.retired.clear();
	}
}

const topologyCache = new Map<string, LiveCollectionTopology>();

export function clearTopologyCache(): void {
	topologyCache.clear();
}

function qdrantHeaders(apiKey: string): Record<string, string> {
	return apiKey ? { "api-key": apiKey } : {};
}

/** Live vector topology for USING suggestions and presets (cached per session). */
export async function fetchCollectionTopology(
	baseUrl: string,
	apiKey: string,
	collection: string,
): Promise<LiveCollectionTopology> {
	const cached = topologyCache.get(collection);
	if (cached) return cached;
	const response = await fetch(
		`${baseUrl}/collections/${encodeURIComponent(collection)}`,
		{ headers: qdrantHeaders(apiKey) },
	);
	if (!response.ok) {
		throw new Error(
			`Collection '${collection}' answered HTTP ${response.status}.`,
		);
	}
	const body = (await response.json()) as {
		result?: {
			config?: { params?: { vectors?: unknown; sparse_vectors?: unknown } };
		};
	};
	const params = body?.result?.config?.params ?? {};
	const vectors = params.vectors as Record<
		string,
		{ multivector_config?: unknown }
	>;
	const sparse = params.sparse_vectors as Record<string, unknown> | undefined;
	const dense: string[] = [];
	const multi: string[] = [];
	if (vectors && typeof vectors === "object") {
		for (const [name, config] of Object.entries(vectors)) {
			if (
				config &&
				typeof config === "object" &&
				"multivector_config" in config
			) {
				multi.push(name);
			} else {
				dense.push(name);
			}
		}
	}
	const topology: LiveCollectionTopology = {
		name: collection,
		dense,
		sparse: sparse && typeof sparse === "object" ? Object.keys(sparse) : [],
		multi,
	};
	topologyCache.set(collection, topology);
	return topology;
}

export async function listCollections(
	baseUrl: string,
	apiKey: string,
	timeoutMs = 8000,
): Promise<string[]> {
	const controller = new AbortController();
	const timer = window.setTimeout(() => controller.abort(), timeoutMs);
	try {
		const response = await fetch(`${baseUrl}/collections`, {
			headers: qdrantHeaders(apiKey),
			signal: controller.signal,
		});
		if (!response.ok) throw new Error(`HTTP ${response.status}`);
		const body = (await response.json()) as {
			result?: { collections?: Array<{ name: string }> };
		};
		return body?.result?.collections?.map((entry) => entry.name) ?? [];
	} finally {
		window.clearTimeout(timer);
	}
}

export type QdrantVerdict =
	| { kind: "ok" }
	| { kind: "http-error"; status: number }
	| { kind: "cors" }
	| { kind: "down" };

/** Qdrant reachability only — never consults the embedder. */
export async function probeQdrant(baseUrl: string): Promise<QdrantVerdict> {
	const withTimeout = async (init: RequestInit): Promise<Response> => {
		const controller = new AbortController();
		const timer = window.setTimeout(() => controller.abort(), 5000);
		try {
			return await fetch(`${baseUrl}/collections`, {
				...init,
				signal: controller.signal,
			});
		} finally {
			window.clearTimeout(timer);
		}
	};
	try {
		const probe = await withTimeout({});
		if (!probe.ok) return { kind: "http-error", status: probe.status };
		return { kind: "ok" };
	} catch {
		// A normal fetch rejects for both a down server and a CORS block.
		// An opaque no-cors probe resolves only when the server is up, so it
		// tells the two cases apart without extra requests on success.
		try {
			const opaque = await withTimeout({ mode: "no-cors" });
			if (opaque.type === "opaque") return { kind: "cors" };
		} catch {
			// Still unreachable.
		}
		return { kind: "down" };
	}
}

export type EmbedVerdict =
	| { probe: "browser" }
	| { probe: "none" }
	| { probe: "http"; ok: boolean; dim: number | null; httpStatus?: number }
	| { probe: "http"; ok: false; dim: null; unreachable: true }
	| { probe: "http"; ok: false; dim: null; empty: true };

/** Embedder model+dim check — never touches Qdrant status. */
export async function probeEmbedder(
	settings: PlaygroundSettings,
): Promise<EmbedVerdict> {
	if (settings.embedProvider === "browser") return { probe: "browser" };
	if (settings.embedProvider === "none") return { probe: "none" };
	try {
		const controller = new AbortController();
		const timer = window.setTimeout(() => controller.abort(), 8000);
		let response: Response;
		try {
			response = await fetch(settings.embedUrl, {
				method: "POST",
				headers: {
					"content-type": "application/json",
					...(settings.embedKey
						? { authorization: `Bearer ${settings.embedKey}` }
						: {}),
				},
				body: JSON.stringify({
					model: settings.embedModel,
					input: ["connection probe"],
				}),
				signal: controller.signal,
			});
		} finally {
			window.clearTimeout(timer);
		}
		if (!response.ok)
			return {
				probe: "http",
				ok: false,
				dim: null,
				httpStatus: response.status,
			};
		const body = (await response.json()) as {
			data?: Array<{ embedding?: number[] }>;
		};
		const dim = body?.data?.[0]?.embedding?.length ?? 0;
		if (!dim) return { probe: "http", ok: false, dim: null, empty: true };
		return { probe: "http", ok: dim === settings.embedDim, dim };
	} catch {
		return { probe: "http", ok: false, dim: null, unreachable: true };
	}
}

/** Shared client pool. */
export const pool = new QdrantClient();
