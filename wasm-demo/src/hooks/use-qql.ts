import { useCallback, useEffect, useRef, useState } from "react"
import init, { analyze, Client, Stmt } from "qql-wasm"
import type {
  AnalysisResult,
  ExecMetrics,
  PlaygroundSettings,
  TenantConfig,
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
    routes: [],
    explain: null,
    error: null,
  }
}

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
  const [response, setResponse] = useState<unknown>(null)
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
  const parseMsRef = useRef(0)
  const analysisRef = useRef(analysis)
  const activeExecutionsRef = useRef(0)
  const retiredClientsRef = useRef<Client[]>([])
  const probeRef = useRef<EmbedProbe>({
    lastEmbedMs: null,
    lastEmbedTexts: 0,
    lastEmbedDim: BROWSER_EMBED_DIM,
  })

  useEffect(() => subscribeBrowserEmbedder(setBrowserStatus), [])
  useEffect(() => {
    settingsRef.current = settings
  }, [settings])
  useEffect(() => {
    analysisRef.current = analysis
  }, [analysis])

  const releaseRetiredClients = useCallback(() => {
    if (activeExecutionsRef.current !== 0) return
    for (const client of retiredClientsRef.current) client.free()
    retiredClientsRef.current = []
  }, [])

  const retireClient = useCallback(
    (client: Client | null) => {
      if (!client) return
      retiredClientsRef.current.push(client)
      releaseRetiredClients()
    },
    [releaseRetiredClients]
  )

  const configureClient = useCallback(
    async (cfg: PlaygroundSettings) => {
      const url = cfg.qdrantUrl.trim() || "http://localhost:6333"
      const key = cfg.qdrantKey.trim() || undefined
      const client = new Client(url, key ?? null)

      try {
        if (cfg.embedProvider === "browser") {
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
      } catch (error) {
        client.free()
        throw error
      }

      const previous = clientRef.current
      clientRef.current = client
      retireClient(previous)
    },
    [retireClient]
  )

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
      retireClient(clientRef.current)
      clientRef.current = null
    }
  }, [configureClient, retireClient])

  const runAnalysis = useCallback(
    (source: string, tenantConfig?: TenantConfig) => {
      if (!ready || !source.trim()) {
        const empty = emptyAnalysis()
        setAnalysis(empty)
        setParseMs(0)
        parseMsRef.current = 0
        return empty
      }

      const t0 = performance.now()
      const finish = (result: AnalysisResult) => {
        const elapsed = performance.now() - t0
        setParseMs(elapsed)
        parseMsRef.current = elapsed
        setAnalysis(result)
        analysisRef.current = result
        return result
      }

      try {
        const baseResult = analyze(source) as AnalysisResult

        if (
          baseResult.valid &&
          tenantConfig?.enabled &&
          tenantConfig.field.trim() &&
          tenantConfig.value.trim()
        ) {
          if (baseResult.statements_count !== 1) {
            return finish({
              ...baseResult,
              valid: false,
              route: null,
              routes: [],
              error: {
                code: "TENANT_MULTI_STATEMENT",
                message:
                  "Tenant isolation currently accepts exactly one statement; split this script and execute each statement separately.",
                start: null,
                end: null,
              },
            })
          }

          try {
            const stmt = new Stmt(source)
            try {
              stmt.injectFilter(
                tenantConfig.field.trim(),
                tenantConfig.op || "=",
                tenantConfig.value.trim()
              )
              if (tenantConfig.shardKey.trim()) {
                stmt.shardKey = tenantConfig.shardKey.trim()
              }
              const route = stmt.compileRoute()
              return finish({
                ...baseResult,
                route,
                routes: [route],
                ast: [stmt.toObject()],
              })
            } finally {
              stmt.free()
            }
          } catch (err) {
            return finish({
              ...baseResult,
              valid: false,
              route: null,
              routes: [],
              error: {
                code: "TENANT_ISOLATION",
                message: `Tenant Injection Error: ${err instanceof Error ? err.message : String(err)}`,
                start: null,
                end: null,
              },
            })
          }
        }

        return finish(baseResult)
      } catch (err) {
        return finish({
          ...emptyAnalysis(),
          error: {
            code: "WASM_ANALYZE",
            message: err instanceof Error ? err.message : String(err),
            start: null,
            end: null,
          },
        })
      }
    },
    [ready]
  )

  const updateSettings = useCallback(
    async (next: PlaygroundSettings) => {
      setSettingsState(next)
      settingsRef.current = next
      saveSettings(next)
      await configureClient(next)
    },
    [configureClient]
  )

  const execute = useCallback(
    async (source: string, tenantConfig?: TenantConfig) => {
      if (!ready) return
      const text = source.trim()
      if (!text) return

      const cfg = settingsRef.current
      setExecuting(true)

      const probe = probeRef.current
      probe.lastEmbedMs = null
      probe.lastEmbedTexts = 0
      let acquiredClient = false

      try {
        if (
          !clientRef.current ||
          (cfg.embedProvider === "browser" && !clientRef.current.hasEmbedder())
        ) {
          await configureClient(cfg)
        }
        const client = clientRef.current
        if (!client) throw new Error("QQL WASM client is not initialized")
        activeExecutionsRef.current += 1
        acquiredClient = true

        const t0 = performance.now()
        let report: unknown

        if (
          tenantConfig?.enabled &&
          tenantConfig.field.trim() &&
          tenantConfig.value.trim()
        ) {
          if (analysisRef.current.statements_count !== 1) {
            throw new Error(
              "Tenant isolation refuses multi-statement execution; split the script and execute each statement separately."
            )
          }
          const stmt = new Stmt(text)
          try {
            stmt.injectFilter(
              tenantConfig.field.trim(),
              tenantConfig.op || "=",
              tenantConfig.value.trim()
            )
            if (tenantConfig.shardKey.trim()) {
              stmt.shardKey = tenantConfig.shardKey.trim()
            }
            report = await client.executeStmt(stmt)
          } finally {
            stmt.free()
          }
        } else {
          report = await client.execute(text)
        }
        const totalMs = performance.now() - t0

        setResponse(report)

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
        setResponse({
          error: message,
          route: analysisRef.current.route ?? null,
        })
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
        if (acquiredClient) {
          activeExecutionsRef.current -= 1
          releaseRetiredClients()
        }
        setExecuting(false)
      }
    },
    [ready, configureClient, releaseRetiredClients]
  )

  return {
    ready,
    initError,
    settings,
    updateSettings,
    analysis,
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
