const MODEL = "Xenova/all-MiniLM-L6-v2";

type ProgressUpdate = {
	status?: string;
	file?: string;
	progress?: number;
};

type TensorLike = {
	tolist(): unknown;
	dispose?: () => void;
};

type FeaturePipeline = (
	input: string[],
	options: { pooling: "mean"; normalize: true },
) => Promise<TensorLike>;

let pipelinePromise: Promise<FeaturePipeline> | null = null;

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

async function loadPipeline(
	setStatus: (message: string) => void,
): Promise<FeaturePipeline> {
	if (!pipelinePromise) {
		pipelinePromise = (async () => {
			setStatus("Loading MiniLM in this browser…");
			const transformers = await import("@huggingface/transformers");
			transformers.env.allowLocalModels = false;
			transformers.env.useBrowserCache = true;

			const progress_callback = (update: ProgressUpdate) => {
				if (update.status === "progress" && update.file) {
					const percent =
						typeof update.progress === "number"
							? ` ${Math.round(update.progress)}%`
							: "";
					setStatus(`Loading ${update.file}${percent}`);
				}
			};

			try {
				return (await transformers.pipeline("feature-extraction", MODEL, {
					device: "webgpu",
					dtype: "q8",
					progress_callback,
				})) as unknown as FeaturePipeline;
			} catch {
				setStatus("WebGPU unavailable; loading the WASM embedding backend…");
				return (await transformers.pipeline("feature-extraction", MODEL, {
					device: "wasm",
					dtype: "q8",
					progress_callback,
				})) as unknown as FeaturePipeline;
			}
		})().catch((error) => {
			pipelinePromise = null;
			throw error;
		});
	}
	return pipelinePromise;
}

export function createBrowserEmbedder(
	setStatus: (message: string) => void,
): (texts: string[]) => Promise<number[][]> {
	return async (texts) => {
		if (texts.length === 0) return [];
		const pipeline = await loadPipeline(setStatus);
		const output = await pipeline(texts, { pooling: "mean", normalize: true });
		try {
			const vectors = normalizeVectors(output.tolist());
			setStatus(`MiniLM ready · ${vectors[0]?.length ?? 384} dimensions`);
			return vectors;
		} finally {
			output.dispose?.();
		}
	};
}
