import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircleIcon,
  BrainCircuitIcon,
  CheckCircle2Icon,
  EraserIcon,
  Loader2Icon,
  MoonIcon,
  PlayIcon,
  Settings2Icon,
  SunIcon,
  ZapIcon,
  ShieldCheckIcon,
  Code2Icon,
  Share2Icon,
  CheckIcon,
  CopyIcon,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useTheme } from "@/components/theme-provider"
import { QueryEditor } from "@/components/playground/query-editor"
import { Inspector } from "@/components/playground/inspector"
import { SettingsDialog } from "@/components/playground/settings-dialog"
import { AuditBar } from "@/components/playground/audit-bar"
import { TenantControl } from "@/components/playground/tenant-sandbox"
import { CodeExporter } from "@/components/playground/code-exporter"
import { useQql } from "@/hooks/use-qql"
import {
  DEFAULT_PRESET_ID,
  PRESETS,
  getPreset,
  type PresetId,
} from "@/lib/presets"
import type { InspectorTab, TenantConfig } from "@/lib/qql-types"
import { BROWSER_EMBED_MODEL } from "@/lib/browser-embedder"

function useDebouncedCallback<T extends (...args: never[]) => void>(
  fn: T,
  delay: number
) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const fnRef = useRef(fn)
  fnRef.current = fn

  return useCallback(
    (...args: Parameters<T>) => {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(() => fnRef.current(...args), delay)
    },
    [delay]
  )
}

function getInitialQuery(): string {
  if (typeof window !== "undefined" && window.location.hash) {
    const match = window.location.hash.match(/#q=(.+)/)
    if (match && match[1]) {
      try {
        return decodeURIComponent(match[1])
      } catch {
        // Fallback to preset
      }
    }
  }
  return getPreset(DEFAULT_PRESET_ID)?.query ?? ""
}

export function App() {
  const { theme, setTheme } = useTheme()
  const {
    ready,
    initError,
    settings,
    updateSettings,
    analysis,
    latencyMs,
    parseMs,
    response,
    executing,
    runAnalysis,
    execute,
    metrics,
    browserStatus,
  } = useQql()

  const [presetId, setPresetId] = useState<PresetId>(DEFAULT_PRESET_ID)
  const [query, setQuery] = useState(getInitialQuery)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [settingsSaving, setSettingsSaving] = useState(false)
  const [codeExporterOpen, setCodeExporterOpen] = useState(false)
  const [copiedLink, setCopiedLink] = useState(false)
  const [copiedQql, setCopiedQql] = useState(false)
  const [activeTab, setActiveTab] = useState<InspectorTab>("plan")
  const [tenantConfig, setTenantConfig] = useState<TenantConfig>({
    enabled: false,
    field: "tenant_id",
    op: "=",
    value: "honeywell",
    shardKey: "honeywell",
  })

  const activePreset = useMemo(() => getPreset(presetId), [presetId])

  const debouncedAnalyze = useDebouncedCallback((src: string) => {
    runAnalysis(src, tenantConfig)
  }, 80)

  useEffect(() => {
    if (ready) runAnalysis(query, tenantConfig)
  }, [ready, query, tenantConfig]) // eslint-disable-line react-hooks/exhaustive-deps

  const [selectedStmtIndex, setSelectedStmtIndex] = useState(0)

  const onQueryChange = (value: string) => {
    setQuery(value)
    setSelectedStmtIndex(0)
    debouncedAnalyze(value)
  }

  const onPresetChange = (id: string | null) => {
    if (!id) return
    const preset = getPreset(id)
    if (!preset) return
    setPresetId(preset.id)
    setQuery(preset.query)
    setSelectedStmtIndex(0)
    runAnalysis(preset.query, tenantConfig)
  }

  const onExecute = useCallback(async () => {
    setActiveTab("response")
    await execute(query, tenantConfig)
  }, [execute, query, tenantConfig])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        e.preventDefault()
        if (ready && analysis.valid && !executing) {
          onExecute()
        }
      }
    }
    window.addEventListener("keydown", handleKeyDown)
    return () => window.removeEventListener("keydown", handleKeyDown)
  }, [ready, analysis.valid, executing, onExecute])

  const onCopyQql = () => {
    if (!query) return
    navigator.clipboard.writeText(query)
    setCopiedQql(true)
    setTimeout(() => setCopiedQql(false), 2000)
  }

  const onShareQuery = () => {
    const url = `${window.location.origin}${window.location.pathname}#q=${encodeURIComponent(query)}`
    navigator.clipboard.writeText(url)
    setCopiedLink(true)
    setTimeout(() => setCopiedLink(false), 2000)
  }

  const status = useMemo(() => {
    if (!ready) return { label: "Loading WASM…", ok: false as boolean | null }
    if (!query.trim()) return { label: "Empty", ok: null }
    if (analysis.valid) {
      return {
        label:
          analysis.statements_count > 1
            ? `${analysis.statements_count} statements`
            : "Valid",
        ok: true,
      }
    }
    return { label: analysis.error?.code ?? "Error", ok: false }
  }, [ready, query, analysis])

  const embedBadge = useMemo(() => {
    if (settings.embedProvider === "none") {
      return { label: "No embed", variant: "outline" as const }
    }
    if (settings.embedProvider === "http") {
      return { label: "HTTP embed", variant: "secondary" as const }
    }
    if (browserStatus.state === "loading") {
      return {
        label: `MiniLM ${Math.round(browserStatus.progress)}%`,
        variant: "secondary" as const,
      }
    }
    if (browserStatus.state === "ready") {
      return {
        label: `MiniLM · ${browserStatus.device ?? "browser"}`,
        variant: "default" as const,
      }
    }
    if (browserStatus.state === "error") {
      return { label: "Embed error", variant: "destructive" as const }
    }
    return { label: "MiniLM…", variant: "secondary" as const }
  }, [settings.embedProvider, browserStatus])

  const toggleTheme = () => {
    const next =
      theme === "dark" ? "light" : theme === "light" ? "dark" : "dark"
    setTheme(next)
  }

  if (initError) {
    return (
      <div className="flex min-h-svh items-center justify-center p-6">
        <Alert variant="destructive" className="max-w-md">
          <AlertCircleIcon />
          <AlertTitle>Failed to load qql-wasm</AlertTitle>
          <AlertDescription>{initError}</AlertDescription>
        </Alert>
      </div>
    )
  }

  return (
    <TooltipProvider>
      <div className="flex h-svh flex-col overflow-hidden bg-background text-foreground">
        <header className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-2 sm:gap-3 sm:px-4">
          <div className="flex items-center gap-2">
            <span className="text-base font-semibold tracking-tight">QQL</span>
            <Badge variant="secondary" className="gap-1 font-mono text-[10px]">
              <ZapIcon className="size-3" />
              WASM
            </Badge>
            <Badge
              variant={embedBadge.variant}
              className="hidden gap-1 font-mono text-[10px] sm:inline-flex"
            >
              <BrainCircuitIcon className="size-3" />
              {embedBadge.label}
            </Badge>
          </div>

          <Separator orientation="vertical" className="hidden h-6 sm:block" />

          <div className="flex min-w-0 flex-1 items-center gap-2">
            <span className="hidden text-xs text-muted-foreground sm:inline">
              Preset
            </span>
            <Select value={presetId} onValueChange={onPresetChange}>
              <SelectTrigger size="sm" className="w-[300px] sm:w-[420px] font-mono text-xs truncate">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="w-[360px] sm:w-[460px]">
                {PRESETS.map((p) => (
                  <SelectItem key={p.id} value={p.id} className="cursor-pointer py-1.5">
                    <div className="flex items-center gap-2 w-full truncate">
                      {p.labelBadge && (
                        <Badge variant="outline" className="font-mono text-[9px] px-1 py-0 h-4 shrink-0 bg-primary/10 border-primary/30 text-primary">
                          {p.labelBadge}
                        </Badge>
                      )}
                      <span className="truncate font-mono text-xs">{p.label}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center gap-1.5 sm:gap-2">
            <Badge
              variant={
                status.ok === true
                  ? "default"
                  : status.ok === false
                    ? "destructive"
                    : "outline"
              }
              className="gap-1"
            >
              {status.ok === true ? (
                <CheckCircle2Icon className="size-3" />
              ) : status.ok === false ? (
                <AlertCircleIcon className="size-3" />
              ) : null}
              {status.label}
            </Badge>

            <span className="hidden font-mono text-[11px] text-muted-foreground tabular-nums sm:inline">
              parse {latencyMs.toFixed(2)} ms
              {metrics?.totalMs != null && metrics.success
                ? ` · exec ${metrics.totalMs.toFixed(0)} ms`
                : ""}
            </span>

            {/* Share Link */}
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="outline"
                    size="icon-sm"
                    onClick={onShareQuery}
                  />
                }
              >
                {copiedLink ? <CheckIcon className="size-3.5 text-emerald-500" /> : <Share2Icon className="size-3.5" />}
              </TooltipTrigger>
              <TooltipContent>{copiedLink ? "Link Copied!" : "Share Query URL Link"}</TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="outline"
                    size="icon-sm"
                    onClick={() => setSettingsOpen(true)}
                  />
                }
              >
                <Settings2Icon />
              </TooltipTrigger>
              <TooltipContent>Settings</TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="outline"
                    size="icon-sm"
                    onClick={toggleTheme}
                  />
                }
              >
                {theme === "dark" ? <SunIcon /> : <MoonIcon />}
              </TooltipTrigger>
              <TooltipContent>Toggle theme</TooltipContent>
            </Tooltip>
          </div>
        </header>

        <main className="min-h-0 flex-1">
          <ResizablePanelGroup orientation="horizontal" className="h-full">
            <ResizablePanel defaultSize={52} minSize={30}>
              <section className="flex h-full min-h-0 flex-col">
                <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b px-3 py-1.5 bg-muted/20">
                  <span className="text-xs font-semibold tracking-wide text-muted-foreground uppercase font-mono">
                    Query editor · {(() => {
                      const r = analysis.routes?.[0] ?? analysis.route
                      const m = r?.path?.match(/\/collections\/([^/]+)/)
                      return m ? m[1] : "sec10k"
                    })()}
                  </span>
                  <div className="flex items-center gap-1.5">
                    {/* Execute right beside editor */}
                    <Button
                      size="sm"
                      disabled={
                        !ready ||
                        !analysis.valid ||
                        executing ||
                        (settings.embedProvider === "browser" &&
                          browserStatus.state === "error")
                      }
                      onClick={onExecute}
                      className="gap-1.5 font-semibold text-xs"
                    >
                      {executing ? (
                        <Loader2Icon className="size-3.5 animate-spin" />
                      ) : (
                        <PlayIcon className="size-3.5" />
                      )}
                      Execute
                    </Button>

                    <Separator orientation="vertical" className="h-5" />

                    <TenantControl
                      tenantConfig={tenantConfig}
                      onUpdateConfig={(next) => {
                        setTenantConfig(next)
                        runAnalysis(query, next)
                      }}
                    />

                    <Button
                      variant="outline"
                      size="sm"
                      onClick={onCopyQql}
                      className="font-mono text-xs gap-1"
                    >
                      {copiedQql ? <CheckIcon className="size-3.5 text-emerald-500" /> : <CopyIcon className="size-3.5 text-primary" />}
                      {copiedQql ? "Copied QQL" : "Copy QQL"}
                    </Button>

                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setCodeExporterOpen(true)}
                      className="font-mono text-xs gap-1"
                    >
                      <Code2Icon className="size-3.5 text-primary" />
                      Copy SDK Code
                    </Button>

                    <Button
                      variant="ghost"
                      size="xs"
                      onClick={() => {
                        setQuery("")
                        runAnalysis("")
                      }}
                      className="text-muted-foreground"
                    >
                      <EraserIcon className="size-3.5" />
                      Clear
                    </Button>
                  </div>
                </div>

                {tenantConfig.enabled && (
                  <div className="flex items-center justify-between gap-2 px-3 py-1.5 bg-emerald-500/10 border-b border-emerald-500/30 text-emerald-600 dark:text-emerald-400 font-mono text-xs shrink-0 select-none">
                    <div className="flex items-center gap-2">
                      <ShieldCheckIcon className="size-4 shrink-0 text-emerald-500" />
                      <span className="font-bold">AST Directives Active:</span>
                      <Badge variant="outline" className="font-mono text-[10px] bg-emerald-500/10 border-emerald-500/40 text-emerald-400">
                        WHERE {tenantConfig.field} {tenantConfig.op} '{tenantConfig.value}'
                      </Badge>
                      {tenantConfig.shardKey.trim() && (
                        <Badge variant="outline" className="font-mono text-[10px] bg-emerald-500/10 border-emerald-500/40 text-emerald-400">
                          SHARD '{tenantConfig.shardKey}'
                        </Badge>
                      )}
                    </div>
                    <span className="text-[10px] text-muted-foreground hidden sm:inline">
                      🔒 Injected at AST layer on execute (Editor query stays pure)
                    </span>
                  </div>
                )}

                <div className="min-h-0 flex-1">
                  {ready ? (
                    <QueryEditor
                      value={query}
                      onChange={onQueryChange}
                      analysis={analysis}
                      onExecute={onExecute}
                      className="h-full"
                    />
                  ) : (
                    <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
                      <Loader2Icon className="size-4 animate-spin" />
                      Loading qql-wasm…
                    </div>
                  )}
                </div>

                {!analysis.valid &&
                  analysis.error?.message &&
                  query.trim() && (
                    <div className="flex shrink-0 items-start gap-2 border-t border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                      <AlertCircleIcon className="mt-0.5 size-3.5 shrink-0" />
                      <span className="font-mono">
                        {analysis.error.code}: {analysis.error.message}
                      </span>
                    </div>
                  )}
              </section>
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel defaultSize={48} minSize={28}>
              <Inspector
                analysis={analysis}
                responseJson={response}
                activeTab={activeTab}
                onTabChange={setActiveTab}
                metrics={metrics}
                parseMs={parseMs}
                browserStatus={browserStatus}
                embedProvider={settings.embedProvider}
                qdrantUrl={settings.qdrantUrl}
                teachingNote={activePreset?.teaching}
                selectedStmtIndex={selectedStmtIndex}
                onSelectStmtIndex={setSelectedStmtIndex}
                tenantConfig={tenantConfig}
                className="h-full"
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </main>

        {/* Compiler Audit Bar */}
        <AuditBar analysis={analysis} query={query} />

        <footer className="flex shrink-0 items-center justify-between gap-2 border-t px-3 py-1.5 text-[11px] text-muted-foreground">
          <span className="min-w-0 truncate">
            {settings.qdrantUrl}
            {" · "}
            {settings.embedProvider === "browser"
              ? `${BROWSER_EMBED_MODEL}${browserStatus.device ? ` · ${browserStatus.device}` : ""}`
              : settings.embedProvider === "http"
                ? settings.embedModel
                : "no embedder"}
            {" · "}
            {(() => {
              const r = analysis.routes?.[0] ?? analysis.route
              const m = r?.path?.match(/\/collections\/([^/]+)/)
              return m ? m[1] : "sec10k"
            })()}
          </span>
          <span className="hidden shrink-0 sm:inline">
            ⌘/Ctrl+Enter execute · d toggles theme
          </span>
        </footer>

        <SettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          settings={settings}
          saving={settingsSaving}
          onSave={async (next) => {
            setSettingsSaving(true)
            try {
              await updateSettings(next)
            } finally {
              setSettingsSaving(false)
            }
          }}
        />



        <CodeExporter
          open={codeExporterOpen}
          onOpenChange={setCodeExporterOpen}
          query={query}
          qdrantUrl={settings.qdrantUrl}
          analysis={analysis}
          selectedStmtIndex={selectedStmtIndex}
        />
      </div>
    </TooltipProvider>
  )
}

export default App
