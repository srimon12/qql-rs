export type QqlToken = {
  kind: string
  text: string
  pos: number
  end: number
  len: number
}

export type QqlError = {
  code?: string
  message?: string
  start?: number
  end?: number
}

export type QqlRoute = {
  method?: string
  path?: string
  payload?: unknown
}

export type AnalysisResult = {
  valid: boolean
  statements_count: number
  tokens: QqlToken[]
  ast: unknown
  route: QqlRoute | null
  explain: string | null
  error: QqlError | null
}

/** Offline-first: browser MiniLM; HTTP is optional override. */
export type EmbedProvider = "browser" | "http" | "none"

export type PlaygroundSettings = {
  qdrantUrl: string
  qdrantKey: string
  embedProvider: EmbedProvider
  /** Used when embedProvider === "http" */
  embedUrl: string
  embedModel: string
  embedDim: number
  embedKey: string
}

export type ExecMetrics = {
  /** Wall-clock timestamp of the run */
  at: number
  /** Last analyze() duration (parse + route + explain) */
  parseMs: number
  statements: number
  valid: boolean
  /** Time spent inside the embedder callback (null if no embed this run) */
  embedMs: number | null
  embedTexts: number
  embedDim: number
  /** Total client.execute wall time */
  totalMs: number
  /**
   * Approx non-embed time (Qdrant REST + plan prep).
   * totalMs - embedMs when embed ran; else totalMs.
   */
  networkMs: number | null
  embedBackend: string
  embedModel: string
  success: boolean
  error?: string
}

export const DEFAULT_SETTINGS: PlaygroundSettings = {
  qdrantUrl: "http://localhost:6333",
  qdrantKey: "",
  embedProvider: "browser",
  embedUrl: "http://127.0.0.1:1234/v1/embeddings",
  // HTTP fallback name (LM Studio / Ollama); browser uses Xenova MiniLM
  embedModel: "text-embedding-all-minilm-l6-v2-embedding",
  embedDim: 384,
  embedKey: "",
}

export const SETTINGS_STORAGE_KEY = "qql-playground-settings-v2"

function migrateProvider(raw: unknown): EmbedProvider {
  if (raw === "browser" || raw === "http" || raw === "none") return raw
  // legacy keys from first playground revision
  if (raw === "openai" || raw === "remote") return "http"
  return "browser"
}

export function loadSettings(): PlaygroundSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY)
    // also try v1 key once
    const legacy = localStorage.getItem("qql-playground-settings")
    const parsed = raw
      ? JSON.parse(raw)
      : legacy
        ? JSON.parse(legacy)
        : null
    if (!parsed) return { ...DEFAULT_SETTINGS }
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
      embedProvider: migrateProvider(parsed.embedProvider),
      embedDim: Number(parsed.embedDim) || 384,
    }
  } catch {
    return { ...DEFAULT_SETTINGS }
  }
}

export function saveSettings(settings: PlaygroundSettings) {
  localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings))
}
