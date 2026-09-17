import { Client, type ExecutionReport, Stmt } from "qql-wasm-current";
import {
	browserModelInfo,
	type PlaygroundSettings,
	type RuntimePolicy,
} from "../core/types";
import { applyRuntimePolicy } from "../core/wasm";
import { createBrowserEmbedder } from "./embedder";

export interface ExecuteOptions {
	/** Batch policy when several statements run in one call. */
	onError?: "stop" | "continue";
}

export class QdrantClient {
	private current: Client | null = null;
	private retired = new Set<Client>();
	private active = 0;

	get ready(): boolean {
		return this.current !== null;
	}

	configure(
		settings: PlaygroundSettings,
		onBrowserEmbedStatus: (message: string) => void,
		onBrowserDims?: (dims: number) => void,
	): string {
		const previous = this.current;
		const next = new Client(settings.qdrantUrl, settings.qdrantKey || null);

		let note: string;
		if (settings.embedProvider === "browser") {
			next.setEmbedder(
				createBrowserEmbedder({
					model: settings.embedBrowserModel,
					device: settings.embedBrowserDevice,
					onStatus: onBrowserEmbedStatus,
					onDims: onBrowserDims,
				}),
			);
			note = `${browserModelInfo(settings.embedBrowserModel).label} loads only when a run needs text embeddings.`;
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

	async execute(
		source: string,
		policy: RuntimePolicy,
		options?: ExecuteOptions,
	): Promise<ExecutionReport> {
		const client = this.current;
		if (!client) throw new Error("Qdrant client is not configured.");
		let statement: Stmt | null = null;
		this.active += 1;
		try {
			if (policy.enabled) {
				statement = new Stmt(source);
				applyRuntimePolicy(statement, policy);
				return await client.executeStmt(statement, options);
			}
			return await client.execute(source, options);
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

function qdrantHeaders(apiKey: string): Record<string, string> {
	return apiKey ? { "api-key": apiKey } : {};
}

export type QdrantVerdict =
	| { kind: "ok" }
	| { kind: "http-error"; status: number }
	| { kind: "cors" }
	| { kind: "down" };

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
		try {
			const opaque = await withTimeout({ mode: "no-cors" });
			if (opaque.type === "opaque") return { kind: "cors" };
		} catch {
			// unreachable
		}
		return { kind: "down" };
	}
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
