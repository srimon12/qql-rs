import { useCallback, useEffect, useRef, useState } from "react"
import init, { analyze, Client } from "qql-wasm"
import type { AnalysisResult, PlaygroundSettings } from "@/lib/qql-types"
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from "@/lib/qql-types"

function emptyAnalysis(): AnalysisResult {
  return {
    valid: false,
    statements_count: 0,
    tokens: [],
    ast: null,
    route: null,
    explain: null,
    error: null,
  }
}

function parseAnalysis(raw: string): AnalysisResult {
  try {
    const data = JSON.parse(raw) as AnalysisResult
    return {
      valid: Boolean(data.valid),
      statements_count: data.statements_count ?? 0,
      tokens: Array.isArray(data.tokens) ? data.tokens : [],
      ast: data.ast ?? null,
      route: data.route ?? null,
      explain: data.explain ?? null,
      error: data.error ?? null,
    }
  } catch {
    return {
      ...emptyAnalysis(),
      error: { code: "PARSE", message: "Failed to parse analyze() result" },
    }
  }
}

export function useQql() {
  const [ready, setReady] = useState(false)
  const [initError, setInitError] = useState<string | null>(null)
  const [settings, setSettingsState] = useState<PlaygroundSettings>(DEFAULT_SETTINGS)
  const [analysis, setAnalysis] = useState<AnalysisResult>(emptyAnalysis)
  const [latencyMs, setLatencyMs] = useState(0)
  const [response, setResponse] = useState<string>("")
  const [executing, setExecuting] = useState(false)
  const clientRef = useRef<Client | null>(null)

  const configureClient = useCallback((cfg: PlaygroundSettings) => {
    const url = cfg.qdrantUrl.trim() || "http://localhost:6333"
    const key = cfg.qdrantKey.trim() || undefined
    const client = new Client(url, key ?? null)

    if (cfg.embedProvider === "openai" || cfg.embedProvider === "remote") {
      const embedUrl = cfg.embedUrl.trim() || "http://localhost:11434/v1/embeddings"
      const model = cfg.embedModel.trim() || "all-minilm:l6-v2"
      const dim = Number(cfg.embedDim) || 384
      const embedKey = cfg.embedKey.trim() || null
      client.setHttpEmbedder(embedUrl, model, dim, embedKey)
    }

    clientRef.current = client
  }, [])

  useEffect(() => {
    let cancelled = false

    ;(async () => {
      try {
        await init()
        if (cancelled) return
        const cfg = loadSettings()
        setSettingsState(cfg)
        configureClient(cfg)
        setReady(true)
      } catch (err) {
        if (!cancelled) {
          setInitError(err instanceof Error ? err.message : String(err))
        }
      }
    })()

    return () => {
      cancelled = true
    }
  }, [configureClient])

  const runAnalysis = useCallback(
    (source: string) => {
      if (!ready) return emptyAnalysis()
      if (!source.trim()) {
        const empty = emptyAnalysis()
        setAnalysis(empty)
        setLatencyMs(0)
        return empty
      }

      const t0 = performance.now()
      const result = parseAnalysis(analyze(source))
      const t1 = performance.now()
      setLatencyMs(t1 - t0)
      setAnalysis(result)
      return result
    },
    [ready]
  )

  const updateSettings = useCallback(
    (next: PlaygroundSettings) => {
      setSettingsState(next)
      saveSettings(next)
      configureClient(next)
    },
    [configureClient]
  )

  const execute = useCallback(
    async (source: string) => {
      if (!ready) return
      const text = source.trim()
      if (!text) return

      if (!clientRef.current) {
        configureClient(settings)
      }

      setExecuting(true)
      setResponse("Executing via QQL WASM Client (embed → Qdrant REST)…")

      try {
        const resJson = await clientRef.current!.execute(text)
        try {
          setResponse(JSON.stringify(JSON.parse(resJson), null, 2))
        } catch {
          setResponse(resJson)
        }
      } catch (err) {
        setResponse(
          JSON.stringify(
            {
              error: String(err),
              note: "If Qdrant or the embedder is not running, open Settings and check URLs.",
              route: analysis.route ?? null,
            },
            null,
            2
          )
        )
      } finally {
        setExecuting(false)
      }
    },
    [ready, settings, configureClient, analysis.route]
  )

  return {
    ready,
    initError,
    settings,
    updateSettings,
    analysis,
    latencyMs,
    response,
    setResponse,
    executing,
    runAnalysis,
    execute,
  }
}
