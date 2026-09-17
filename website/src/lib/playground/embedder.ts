import type { BrowserEmbedDevice } from "./types";

type ProgressUpdate = {
	status?: string;
	file?: string;
	progress?: number;
	loaded?: number;
	total?: number;
};

type TensorLike = {
	tolist(): unknown;
	dispose?: () => void;
};

type FeaturePipeline = (
	input: string[],
	options: { pooling: "mean"; normalize: true },
) => Promise<TensorLike>;

export interface BrowserEmbedderOptions {
	/** Transformers.js model id, e.g. `Xenova/bge-small-en-v1.5`. */
	model: string;
	/** `auto` probes WebGPU and falls back to the WASM backend. */
	device: BrowserEmbedDevice;
	onStatus: (message: string) => void;
	/** Called once with the real tensor width after the first embedding. */
	onDims?: (dims: number) => void;
}

interface LoadedPipeline {
	pipeline: FeaturePipeline;
	/** Backend that actually loaded (`webgpu` or `wasm`). */
	backend: "webgpu" | "wasm";
}

const pipelines = new Map<string, Promise<LoadedPipeline>>();

function formatMb(bytes: number | undefined): string {
	if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes <= 0)
		return "";
	return ` ${(bytes / 1_048_576).toFixed(1)} MB`;
}

function normalizeVectors(value: unknown): number[][] {
	if (!Array.isArray(value)) {
		throw new Error(
			"The browser embedding model returned an unexpected tensor.",
		);
	}
	if (value.length === 0) return [];
	if (Array.isArray(value[0])) return value as number[][];
	return [value as number[]];
}

async function loadPipeline({
	model,
	device,
	onStatus,
}: BrowserEmbedderOptions): Promise<LoadedPipeline> {
	const key = `${model}|${device}`;
	const cached = pipelines.get(key);
	if (cached) return cached;

	const attempts: Array<"webgpu" | "wasm"> =
		device === "auto" ? ["webgpu", "wasm"] : [device];

	const loading = (async () => {
		const transformers = await import("@huggingface/transformers");
		transformers.env.allowLocalModels = false;
		transformers.env.useBrowserCache = true;

		const progress_callback = (update: ProgressUpdate) => {
			if (update.status === "progress" && update.file) {
				onStatus(
					`Downloading ${update.file}${formatMb(update.loaded)}${typeof update.progress === "number" ? ` · ${Math.round(update.progress)}%` : ""}`,
				);
			} else if (update.status === "ready") {
				onStatus(
					`Model ready on the ${attempts.at(-1) === "wasm" ? "WASM" : "WebGPU"} backend`,
				);
			}
		};

		let lastError: unknown;
		for (const [index, backend] of attempts.entries()) {
			try {
				onStatus(
					backend === "webgpu"
						? `Loading ${model} on WebGPU…`
						: `Loading ${model} on the WASM backend…`,
				);
				const pipeline = (await transformers.pipeline(
					"feature-extraction",
					model,
					{ device: backend, dtype: "q8", progress_callback },
				)) as unknown as FeaturePipeline;
				return { pipeline, backend };
			} catch (error) {
				lastError = error;
				if (index < attempts.length - 1) {
					onStatus("WebGPU unavailable; falling back to the WASM backend…");
				}
			}
		}
		throw lastError instanceof Error
			? lastError
			: new Error(`Could not load ${model} in this browser.`);
	})().catch((error) => {
		pipelines.delete(key);
		throw error;
	});

	pipelines.set(key, loading);
	return loading;
}

export function createBrowserEmbedder(
	options: BrowserEmbedderOptions,
): (texts: string[]) => Promise<number[][]> {
	const { onStatus, onDims, model } = options;
	let reportedDims = false;
	return async (texts) => {
		if (texts.length === 0) return [];
		const { pipeline, backend } = await loadPipeline(options);
		const output = await pipeline(texts, { pooling: "mean", normalize: true });
		try {
			const vectors = normalizeVectors(output.tolist());
			const dims = vectors[0]?.length ?? 0;
			if (!reportedDims) {
				reportedDims = true;
				if (dims > 0) onDims?.(dims);
			}
			onStatus(`${model} ready · ${dims} dims · ${backend}`);
			return vectors;
		} finally {
			output.dispose?.();
		}
	};
}
