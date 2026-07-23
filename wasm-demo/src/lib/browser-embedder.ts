/**
 * In-browser dense embeddings via Transformers.js (lazy-loaded).
 * Same family as SEC 10-K demo: all-MiniLM-L6-v2 → 384-d cosine vectors.
 */

/** ONNX export of sentence-transformers/all-MiniLM-L6-v2 (384-d). */
export const BROWSER_EMBED_MODEL = "Xenova/all-MiniLM-L6-v2"
export const BROWSER_EMBED_DIM = 384

export type EmbedDevice = "webgpu" | "wasm"

export type BrowserEmbedderStatus = {
  state: "idle" | "loading" | "ready" | "error"
  model: string
  dim: number
  device: EmbedDevice | null
  loadMs: number | null
  progress: number
  statusText: string
  error: string | null
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type FeaturePipe = (texts: string | string[], opts?: Record<string, unknown>) => Promise<any>

type LoadListener = (status: BrowserEmbedderStatus) => void

let extractor: FeaturePipe | null = null
let loadPromise: Promise<FeaturePipe> | null = null
let deviceUsed: EmbedDevice | null = null
let loadMs: number | null = null
let status: BrowserEmbedderStatus = {
  state: "idle",
  model: BROWSER_EMBED_MODEL,
  dim: BROWSER_EMBED_DIM,
  device: null,
  loadMs: null,
  progress: 0,
  statusText: "Not loaded",
  error: null,
}

const listeners = new Set<LoadListener>()

function emit() {
  for (const l of listeners) l({ ...status })
}

export function getBrowserEmbedderStatus(): BrowserEmbedderStatus {
  return { ...status }
}

export function subscribeBrowserEmbedder(listener: LoadListener): () => void {
  listeners.add(listener)
  listener({ ...status })
  return () => {
    listeners.delete(listener)
  }
}

function setStatus(partial: Partial<BrowserEmbedderStatus>) {
  status = { ...status, ...partial }
  emit()
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function onProgress(info: any) {
  if (info && typeof info.progress === "number") {
    setStatus({
      progress: Math.min(100, Math.max(0, info.progress)),
      statusText: info.file
        ? `Downloading ${String(info.file)}…`
        : "Downloading model…",
    })
  } else if (info?.status === "ready") {
    setStatus({ progress: 100, statusText: "Model ready" })
  } else if (typeof info?.status === "string") {
    setStatus({ statusText: info.status })
  }
}

async function tryLoad(
  pipelineFn: typeof import("@huggingface/transformers").pipeline,
  device: EmbedDevice
): Promise<FeaturePipe> {
  return pipelineFn("feature-extraction", BROWSER_EMBED_MODEL, {
    device,
    dtype: device === "webgpu" ? "fp32" : "q8",
    progress_callback: onProgress,
  }) as Promise<FeaturePipe>
}

/**
 * Load (or reuse) the MiniLM pipeline. Prefers WebGPU, falls back to WASM.
 * Transformers.js is dynamically imported so the ORT WASM is not in the main chunk.
 */
export async function ensureBrowserEmbedder(): Promise<FeaturePipe> {
  if (extractor) return extractor
  if (loadPromise) return loadPromise

  loadPromise = (async () => {
    setStatus({
      state: "loading",
      progress: 0,
      statusText: "Loading Transformers.js…",
      error: null,
    })

    const t0 = performance.now()

    const { env, pipeline } = await import("@huggingface/transformers")
    env.allowLocalModels = false

    setStatus({ statusText: "Loading all-MiniLM-L6-v2…" })

    let pipe: FeaturePipe
    let device: EmbedDevice = "wasm"

    let hasWebGPU = false
    if (typeof navigator !== "undefined" && "gpu" in navigator && navigator.gpu) {
      try {
        const adapter = await navigator.gpu.requestAdapter()
        hasWebGPU = Boolean(adapter)
      } catch {
        hasWebGPU = false
      }
    }

    try {
      if (hasWebGPU) {
        try {
          pipe = await tryLoad(pipeline, "webgpu")
          device = "webgpu"
        } catch (gpuErr) {
          console.warn("WebGPU init failed, falling back to WASM:", gpuErr)
          setStatus({ statusText: "WebGPU unavailable, using WASM…" })
          pipe = await tryLoad(pipeline, "wasm")
          device = "wasm"
        }
      } else {
        pipe = await tryLoad(pipeline, "wasm")
        device = "wasm"
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      setStatus({
        state: "error",
        error: message,
        statusText: "Failed to load model",
        progress: 0,
      })
      loadPromise = null
      throw err
    }

    const elapsed = performance.now() - t0
    extractor = pipe
    deviceUsed = device
    loadMs = elapsed

    setStatus({
      state: "ready",
      device,
      loadMs: elapsed,
      progress: 100,
      statusText: `Ready (${device})`,
      error: null,
    })

    return pipe
  })()

  return loadPromise
}

export type EmbedBatchResult = {
  vectors: number[][]
  embedMs: number
  texts: number
  dim: number
  device: EmbedDevice | null
  model: string
}

/**
 * Embed a batch of texts → 384-d L2-normalized vectors (mean pooling).
 */
export async function embedTexts(texts: string[]): Promise<EmbedBatchResult> {
  if (texts.length === 0) {
    return {
      vectors: [],
      embedMs: 0,
      texts: 0,
      dim: BROWSER_EMBED_DIM,
      device: deviceUsed,
      model: BROWSER_EMBED_MODEL,
    }
  }

  const pipe = await ensureBrowserEmbedder()
  const t0 = performance.now()
  const output = await pipe(texts, { pooling: "mean", normalize: true })
  const embedMs = performance.now() - t0

  const listed = output.tolist() as number[] | number[][]
  let vectors: number[][]
  if (texts.length === 1) {
    if (Array.isArray(listed[0])) {
      vectors = listed as number[][]
    } else {
      vectors = [listed as number[]]
    }
  } else {
    vectors = listed as number[][]
  }

  if (typeof output.dispose === "function") {
    output.dispose()
  }

  return {
    vectors,
    embedMs,
    texts: texts.length,
    dim: vectors[0]?.length ?? BROWSER_EMBED_DIM,
    device: deviceUsed,
    model: BROWSER_EMBED_MODEL,
  }
}

/** Bound callback for qql-wasm Client.setEmbedder */
export async function browserEmbedderFn(texts: string[]): Promise<number[][]> {
  const { vectors } = await embedTexts(texts)
  return vectors
}

export function getBrowserEmbedMeta() {
  return {
    model: BROWSER_EMBED_MODEL,
    dim: BROWSER_EMBED_DIM,
    device: deviceUsed,
    loadMs,
    ready: extractor !== null,
  }
}
