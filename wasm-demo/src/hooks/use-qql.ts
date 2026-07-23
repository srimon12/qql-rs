import { useCallback, useEffect, useRef, useState } from "react"
import init, { analyze, Client } from "qql-wasm"
import type {
  AnalysisResult,
  ExecMetrics,
  PlaygroundSettings,
} from "@/lib/qql-types"
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from "@/lib/qql-types"
import {
  BROWSER_EMBED_DIM,
  BROWSER_EMBED_MODEL,
  browserEmbedderFn,
  getBrowserEmbedMeta,
  subscribeBrowserEmbedder,
  type BrowserEmbedderStatus,
} from "@/lib/browser-embedder"

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

/** Mutable slot so setEmbedder can report timings without rebinding constantly. */
type EmbedProbe = {
  lastEmbedMs: number | null
  lastEmbedTexts: number
  lastEmbedDim: number
}

export function useQql() {
  const [ready, setReady] = useState(false)
  const [initError, setInitError] = useState<string | null>(null)
  const [settings, setSettingsState] =
    useState<PlaygroundSettings>(DEFAULT_SETTINGS)
  const [analysis, setAnalysis] = useState<AnalysisResult>(emptyAnalysis)
  const [parseMs, setParseMs] = useState(0)
  const [response, setResponse] = useState<string>("")
  const [executing, setExecuting] = useState(false)
  const [metrics, setMetrics] = useState<ExecMetrics | null>(null)
  const [browserStatus, setBrowserStatus] = useState<BrowserEmbedderStatus>(
    () => ({
      state: "idle",
      model: BROWSER_EMBED_MODEL,
      dim: BROWSER_EMBED_DIM,
      device: null,
      loadMs: null,
      progress: 0,
      statusText: "Not loaded",
      error: null,
    })
  )

  const clientRef = useRef<Client | null>(null)
  const settingsRef = useRef(settings)
  settingsRef.current = settings
  const parseMsRef = useRef(0)
  const analysisRef = useRef(analysis)
  analysisRef.current = analysis
  const probeRef = useRef<EmbedProbe>({
    lastEmbedMs: null,
    lastEmbedTexts: 0,
    lastEmbedDim: BROWSER_EMBED_DIM,
  })

  useEffect(() => subscribeBrowserEmbedder(setBrowserStatus), [])

  const configureClient = useCallback(async (cfg: PlaygroundSettings) => {
    const url = cfg.qdrantUrl.trim() || "http://localhost:6333"
    const key = cfg.qdrantKey.trim() || undefined
    const client = new Client(url, key ?? null)

    if (cfg.embedProvider === "browser") {
      // Lazy: MiniLM downloads on first embed call, not on page load
      const probe = probeRef.current
      client.setEmbedder(async (texts: string[]) => {
        const t0 = performance.now()
        const vectors = await browserEmbedderFn(texts)
        probe.lastEmbedMs = performance.now() - t0
        probe.lastEmbedTexts = texts.length
        probe.lastEmbedDim = vectors[0]?.length ?? BROWSER_EMBED_DIM
        return vectors
      })
    } else if (cfg.embedProvider === "http") {
      const embedUrl =
        cfg.embedUrl.trim() || "http://localhost:11434/v1/embeddings"
      const model = cfg.embedModel.trim() || "all-minilm:l6-v2"
      const dim = Number(cfg.embedDim) || 384
      const embedKey = cfg.embedKey.trim() || null
      client.setHttpEmbedder(embedUrl, model, dim, embedKey)
    }
    // "none" → no embedder

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
        try {
          await configureClient(cfg)
        } catch (embedErr) {
          // WASM still usable for offline analyze; embed may fail later
          console.warn("Embedder init:", embedErr)
        }
        if (!cancelled) setReady(true)
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
        setParseMs(0)
        parseMsRef.current = 0
        return empty
      }

      const t0 = performance.now()
      const result = parseAnalysis(analyze(source))
      const elapsed = performance.now() - t0
      setParseMs(elapsed)
      parseMsRef.current = elapsed
      setAnalysis(result)
      return result
    },
    [ready]
  )

  const updateSettings = useCallback(
    async (next: PlaygroundSettings) => {
      setSettingsState(next)
      saveSettings(next)
      await configureClient(next)
    },
    [configureClient]
  )

  const execute = useCallback(
    async (source: string) => {
      if (!ready) return
      const text = source.trim()
      if (!text) return

      const cfg = settingsRef.current

      setExecuting(true)
      setResponse(
        cfg.embedProvider === "browser"
          ? "Executing (in-browser MiniLM → Qdrant REST)…"
          : cfg.embedProvider === "http"
            ? "Executing (HTTP embedder → Qdrant REST)…"
            : "Executing (no embedder → Qdrant REST)…"
      )

      // Reset embed probe for this run
      const probe = probeRef.current
      probe.lastEmbedMs = null
      probe.lastEmbedTexts = 0

      try {
        if (!clientRef.current) {
          await configureClient(cfg)
        }

        if (cfg.embedProvider === "browser" && !clientRef.current!.hasEmbedder()) {
          await configureClient(cfg)
        }

        const t0 = performance.now()
        const resJson = await clientRef.current!.execute(text)
        const totalMs = performance.now() - t0

        try {
          setResponse(JSON.stringify(JSON.parse(resJson), null, 2))
        } catch {
          setResponse(resJson)
        }

        const meta = getBrowserEmbedMeta()
        const embedMs = probe.lastEmbedMs
        const networkMs =
          embedMs != null ? Math.max(0, totalMs - embedMs) : totalMs

        setMetrics({
          at: Date.now(),
          parseMs: parseMsRef.current,
          statements: analysisRef.current.statements_count,
          valid: analysisRef.current.valid,
          embedMs,
          embedTexts: probe.lastEmbedTexts,
          embedDim: probe.lastEmbedDim || cfg.embedDim || BROWSER_EMBED_DIM,
          totalMs,
          networkMs,
          embedBackend:
            cfg.embedProvider === "browser"
              ? (meta.device ?? "browser")
              : cfg.embedProvider === "http"
                ? "http"
                : "none",
          embedModel:
            cfg.embedProvider === "browser"
              ? BROWSER_EMBED_MODEL
              : cfg.embedProvider === "http"
                ? cfg.embedModel
                : "—",
          success: true,
        })
      } catch (err) {
        const message = String(err)
        setResponse(
          JSON.stringify(
            {
              error: message,
              note:
                cfg.embedProvider === "browser"
                  ? "Browser MiniLM failed or Qdrant is unreachable. Check the Metrics tab and Settings (Qdrant URL)."
                  : "If Qdrant or the HTTP embedder is not running, open Settings.",
              route: analysisRef.current.route ?? null,
            },
            null,
            2
          )
        )
        setMetrics({
          at: Date.now(),
          parseMs: parseMsRef.current,
          statements: analysisRef.current.statements_count,
          valid: analysisRef.current.valid,
          embedMs: probe.lastEmbedMs,
          embedTexts: probe.lastEmbedTexts,
          embedDim: probe.lastEmbedDim || BROWSER_EMBED_DIM,
          totalMs: 0,
          networkMs: null,
          embedBackend: cfg.embedProvider,
          embedModel:
            cfg.embedProvider === "browser"
              ? BROWSER_EMBED_MODEL
              : cfg.embedModel,
          success: false,
          error: message,
        })
      } finally {
        setExecuting(false)
      }
    },
    [ready, configureClient]
  )

  return {
    ready,
    initError,
    settings,
    updateSettings,
    analysis,
    /** alias used by App toolbar */
    latencyMs: parseMs,
    parseMs,
    response,
    setResponse,
    executing,
    runAnalysis,
    execute,
    metrics,
    browserStatus,
  }
}
