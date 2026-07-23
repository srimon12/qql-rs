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

export type EmbedProvider = "openai" | "remote" | "none"

export type PlaygroundSettings = {
  qdrantUrl: string
  qdrantKey: string
  embedProvider: EmbedProvider
  embedUrl: string
  embedModel: string
  embedDim: number
  embedKey: string
}

export const DEFAULT_SETTINGS: PlaygroundSettings = {
  qdrantUrl: "http://localhost:6333",
  qdrantKey: "",
  embedProvider: "openai",
  embedUrl: "http://127.0.0.1:1234/v1/embeddings",
  embedModel: "text-embedding-all-minilm-l6-v2-embedding",
  embedDim: 384,
  embedKey: "",
}

export const SETTINGS_STORAGE_KEY = "qql-playground-settings"

export function loadSettings(): PlaygroundSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY)
    if (!raw) return { ...DEFAULT_SETTINGS }
    return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_SETTINGS }
  }
}

export function saveSettings(settings: PlaygroundSettings) {
  localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings))
}
