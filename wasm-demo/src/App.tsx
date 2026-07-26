import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircleIcon,
  BookOpenIcon,
  BrainCircuitIcon,
  CheckCircle2Icon,
  CheckIcon,
  ChevronRightIcon,
  Code2Icon,
  CopyIcon,
  EraserIcon,
  Loader2Icon,
  MoonIcon,
  PlayIcon,
  Settings2Icon,
  Share2Icon,
  SunIcon,
  ZapIcon,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
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
import { PolicyControl } from "@/components/playground/policy-guardrails"
import { CodeExporter } from "@/components/playground/code-exporter"
import { PresetBrowser } from "@/components/playground/preset-browser"
import { useQql } from "@/hooks/use-qql"
import {
  DEFAULT_PRESET_ID,
  getPreset,
  getCategory,
  type PresetId,
} from "@/lib/presets"
import type {
  AnalysisResult,
  InspectorTab,
  PlaygroundSettings,
  PolicyConfig,
} from "@/lib/qql-types"
import {
  BROWSER_EMBED_MODEL,
  type BrowserEmbedderStatus,
} from "@/lib/browser-embedder"

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => {
    if (typeof window === "undefined") return false
    return window.matchMedia(query).matches
  })
  useEffect(() => {
    const mql = window.matchMedia(query)
    const handler = (e: MediaQueryListEvent) => setMatches(e.matches)
    mql.addEventListener("change", handler)
    return () => mql.removeEventListener("change", handler)
  }, [query])
  return matches
}

function useDebouncedCallback<T extends (...args: never[]) => void>(
  fn: T,
  delay: number
) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const fnRef = useRef(fn)
  useEffect(() => {
    fnRef.current = fn
  }, [fn])
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
        // hash decode failed, fall through
      }
    }
  }
  return getPreset(DEFAULT_PRESET_ID)?.query ?? ""
}

function formatPolicyValue(config: PolicyConfig): string {
  if (config.valueType === "string") return `'${config.value}'`
  if (config.valueType === "boolean") return config.value
  return config.value
}

type EditorPanelProps = {
  ready: boolean
  query: string
  analysis: AnalysisResult
  executing: boolean
  settings: PlaygroundSettings
  browserStatus: BrowserEmbedderStatus
  onQueryChange: (value: string) => void
  onExecute: () => void
  onCopyQql: () => void
  copiedQql: boolean
  onCodeExport: () => void
  onClear: () => void
  collectionName: string
}

function EditorPanel({
  ready,
  query,
  analysis,
  executing,
  settings,
  browserStatus,
  onQueryChange,
  onExecute,
  onCopyQql,
  copiedQql,
  onCodeExport,
  onClear,
  collectionName,
}: EditorPanelProps) {
  return (
    <section className="flex h-full min-h-0 flex-col" aria-label="Query editor">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b bg-muted/20 px-3 py-1.5">
        <span className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          Source
          <span className="font-mono font-normal text-foreground/70 normal-case">
            {" · "}
            {collectionName}
          </span>
        </span>
        <div className="flex items-center gap-1.5">
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
            className="gap-1.5 rounded-lg text-xs font-semibold"
          >
            {executing ? (
              <Loader2Icon className="size-3.5 animate-spin" />
            ) : (
              <PlayIcon className="size-3.5" />
            )}
            Execute
          </Button>

          <Separator orientation="vertical" className="h-5" />

          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={onCopyQql}
                  aria-label="Copy QQL to clipboard"
                />
              }
            >
              {copiedQql ? (
                <CheckIcon className="size-3.5 text-emerald-500" />
              ) : (
                <CopyIcon className="size-3.5" />
              )}
            </TooltipTrigger>
            <TooltipContent>
              {copiedQql ? "Copied QQL" : "Copy QQL"}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={onCodeExport}
                  aria-label="Export as SDK code"
                />
              }
            >
              <Code2Icon className="size-3.5" />
            </TooltipTrigger>
            <TooltipContent>Copy SDK code</TooltipContent>
          </Tooltip>

          <Separator orientation="vertical" className="h-5" />

          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={onClear}
                  aria-label="Clear query"
                  className="text-muted-foreground hover:text-foreground"
                />
              }
            >
              <EraserIcon className="size-3.5" />
            </TooltipTrigger>
            <TooltipContent>Clear</TooltipContent>
          </Tooltip>
        </div>
      </div>

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
            Loading qql-wasm
          </div>
        )}
      </div>

      {!analysis.valid && analysis.error?.message && query.trim() && (
        <div className="flex shrink-0 items-start gap-2 border-t border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          <AlertCircleIcon className="mt-0.5 size-3.5 shrink-0" />
          <span className="font-mono">
            {analysis.error.code}: {analysis.error.message}
          </span>
        </div>
      )}
    </section>
  )
}

export function App() {
  const { theme, setTheme } = useTheme()
  const isDesktop = useMediaQuery("(min-width: 768px)")
  const [presetBrowserOpen, setPresetBrowserOpen] = useState(false)

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
  const [policyConfig, setPolicyConfig] = useState<PolicyConfig>({
    enabled: false,
    field: "tenant_id",
    op: "=",
    value: "honeywell",
    valueType: "string",
    shardKey: "honeywell",
  })

  const activePreset = useMemo(() => getPreset(presetId), [presetId])
  const activeCategory = useMemo(
    () => (activePreset ? getCategory(activePreset.category) : undefined),
    [activePreset]
  )

  const debouncedAnalyze = useDebouncedCallback((src: string) => {
    runAnalysis(src, policyConfig)
  }, 80)

  useEffect(() => {
    if (ready) runAnalysis(query, policyConfig)
  }, [ready]) // eslint-disable-line react-hooks/exhaustive-deps

  const [selectedStmtIndex, setSelectedStmtIndex] = useState(0)

  const onQueryChange = (value: string) => {
    setQuery(value)
    setSelectedStmtIndex(0)
    debouncedAnalyze(value)
  }

  const onPresetChange = (id: PresetId) => {
    const preset = getPreset(id)
    if (!preset) return
    setPresetId(preset.id)
    setQuery(preset.query)
    setSelectedStmtIndex(0)
    runAnalysis(preset.query, policyConfig)
  }

  const onExecute = useCallback(async () => {
    setActiveTab("response")
    await execute(query, policyConfig)
  }, [execute, query, policyConfig])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        const tag = (e.target as HTMLElement)?.tagName
        const isEditable =
          tag === "INPUT" ||
          tag === "TEXTAREA" ||
          (e.target as HTMLElement)?.isContentEditable
        if (!isEditable) {
          e.preventDefault()
          setPresetBrowserOpen(true)
        }
        return
      }
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
    if (!ready) return { label: "Loading WASM", ok: null as boolean | null }
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
    return { label: "MiniLM", variant: "secondary" as const }
  }, [settings.embedProvider, browserStatus])

  const toggleTheme = () => {
    const next =
      theme === "dark" ? "light" : theme === "light" ? "dark" : "dark"
    setTheme(next)
  }

  const collectionName = useMemo(() => {
    const r = analysis.routes?.[0] ?? analysis.route
    const m = r?.path?.match(/\/collections\/([^/]+)/)
    return m ? m[1] : "sec10k"
  }, [analysis])

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
        {/* Primary header */}
        <header
          className="flex shrink-0 items-center gap-2 border-b px-2.5 py-1.5 sm:gap-3 sm:px-4 sm:py-2"
          aria-label="Workbench header"
        >
          <div className="flex shrink-0 items-center gap-2">
            <span
              className="flex size-6 items-center justify-center rounded-lg bg-primary text-[10px] leading-none font-bold text-primary-foreground"
              aria-hidden
            >
              Q
            </span>
            <span className="hidden text-sm font-semibold tracking-tight sm:inline">
              QQL Workbench
            </span>
          </div>

          <Separator orientation="vertical" className="hidden h-5 sm:block" />

          <div className="flex shrink-0 items-center gap-1.5">
            <Badge
              variant="secondary"
              className="h-5 gap-1 font-mono text-[10px]"
            >
              <ZapIcon className="size-2.5" />
              WASM
            </Badge>
            <Badge
              variant={embedBadge.variant}
              className="hidden h-5 gap-1 font-mono text-[10px] sm:inline-flex"
            >
              <BrainCircuitIcon className="size-2.5" />
              {embedBadge.label}
            </Badge>
          </div>

          <div className="flex-1" />

          <div className="flex items-center gap-1 sm:gap-1.5">
            <Badge
              variant={
                status.ok === true
                  ? "default"
                  : status.ok === false
                    ? "destructive"
                    : "outline"
              }
              className="h-6 gap-1 text-[10px]"
            >
              {status.ok === true ? (
                <CheckCircle2Icon className="size-2.5" />
              ) : status.ok === false ? (
                <AlertCircleIcon className="size-2.5" />
              ) : null}
              {status.label}
            </Badge>

            <span className="hidden font-mono text-[10px] text-muted-foreground tabular-nums sm:inline">
              parse {latencyMs.toFixed(1)} ms
              {metrics?.totalMs != null && metrics.success
                ? ` · exec ${metrics.totalMs.toFixed(0)} ms`
                : ""}
            </span>

            <Separator orientation="vertical" className="hidden h-5 sm:block" />

            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={onShareQuery}
                    aria-label="Share query URL"
                  />
                }
              >
                {copiedLink ? (
                  <CheckIcon className="size-3.5 text-emerald-500" />
                ) : (
                  <Share2Icon className="size-3.5" />
                )}
              </TooltipTrigger>
              <TooltipContent>
                {copiedLink ? "Link copied" : "Share query URL"}
              </TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setSettingsOpen(true)}
                    aria-label="Settings"
                  />
                }
              >
                <Settings2Icon className="size-3.5" />
              </TooltipTrigger>
              <TooltipContent>Settings</TooltipContent>
            </Tooltip>

            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={toggleTheme}
                    aria-label="Toggle color theme"
                  />
                }
              >
                {theme === "dark" ? (
                  <SunIcon className="size-3.5" />
                ) : (
                  <MoonIcon className="size-3.5" />
                )}
              </TooltipTrigger>
              <TooltipContent>Toggle theme</TooltipContent>
            </Tooltip>
          </div>
        </header>

        {/* Scenario bar */}
        <div
          className="flex shrink-0 items-center gap-2 border-b bg-muted/20 px-2.5 py-1.5 sm:px-4"
          role="toolbar"
          aria-label="Scenario controls"
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPresetBrowserOpen(true)}
            className="h-7 shrink-0 gap-1.5 rounded-lg text-xs"
            aria-label="Explore capabilities and query presets"
          >
            <BookOpenIcon className="size-3.5" />
            <span className="hidden sm:inline">Explore capabilities</span>
            <span className="sm:hidden">Explore</span>
          </Button>

          {activePreset && (
            <>
              <Separator orientation="vertical" className="h-5" />
              <button
                onClick={() => setPresetBrowserOpen(true)}
                className="flex min-w-0 items-center gap-2 rounded-lg px-2 py-1 text-left text-xs transition-colors hover:bg-accent/50"
                aria-label={`Active scenario: ${activePreset.label}. Click to open capability library.`}
              >
                {activePreset.labelBadge && (
                  <Badge
                    variant="outline"
                    className="h-4 shrink-0 border-primary/30 bg-primary/10 px-1 py-0 font-mono text-[9px] text-primary"
                  >
                    {activePreset.labelBadge}
                  </Badge>
                )}
                <span className="truncate font-medium">
                  {activePreset.label}
                </span>
                <span className="hidden max-w-[28rem] truncate text-muted-foreground lg:inline">
                  {activePreset.description}
                </span>
                <span className="hidden text-muted-foreground sm:inline">
                  <ChevronRightIcon className="size-3" />
                </span>
              </button>

              <div className="hidden items-center gap-2 text-[10px] text-muted-foreground md:flex">
                {activeCategory && (
                  <Badge
                    variant="outline"
                    className="h-5 rounded-md font-mono text-[9px]"
                  >
                    {activeCategory.label}
                  </Badge>
                )}
                <span>{activePreset.dataset}</span>
                <span className="capitalize">{activePreset.complexity}</span>
              </div>

              <span className="hidden flex-1 sm:block" />

              <div className="hidden items-center gap-1 sm:flex">
                <Separator orientation="vertical" className="h-5" />
              </div>
            </>
          )}

          <div className="flex-1" />

          <PolicyControl
            config={policyConfig}
            onUpdateConfig={(next) => {
              setPolicyConfig(next)
              runAnalysis(query, next)
            }}
          />
        </div>

        {/* Policy enforcement strip */}
        {policyConfig.enabled && (
          <div className="flex shrink-0 items-center gap-2 border-b bg-emerald-500/5 px-2.5 py-1 sm:px-4">
            <span className="shrink-0 text-[10px] font-semibold tracking-wider text-emerald-600 uppercase dark:text-emerald-400">
              Host policy enforced
            </span>
            <Badge
              variant="outline"
              className="h-5 rounded-md border-emerald-500/30 bg-emerald-500/10 font-mono text-[10px] text-emerald-600 dark:text-emerald-400"
            >
              WHERE {policyConfig.field} {policyConfig.op}{" "}
              {formatPolicyValue(policyConfig)}
            </Badge>
            {policyConfig.shardKey.trim() && (
              <Badge
                variant="outline"
                className="h-5 rounded-md border-emerald-500/30 bg-emerald-500/10 font-mono text-[10px] text-emerald-600 dark:text-emerald-400"
              >
                SHARD '{policyConfig.shardKey}'
              </Badge>
            )}
            <span className="hidden text-[10px] text-muted-foreground sm:inline">
              Injected at AST layer on execute · Editor query stays pure
            </span>
          </div>
        )}

        {/* Main workspace */}
        <main className="flex min-h-0 flex-1 flex-col" aria-label="Workspace">
          {isDesktop ? (
            <ResizablePanelGroup
              orientation="horizontal"
              className="flex-1"
            >
              <ResizablePanel defaultSize={52} minSize={30}>
                <EditorPanel
                  ready={ready}
                  query={query}
                  analysis={analysis}
                  executing={executing}
                  settings={settings}
                  browserStatus={browserStatus}
                  onQueryChange={onQueryChange}
                  onExecute={onExecute}
                  onCopyQql={onCopyQql}
                  copiedQql={copiedQql}
                  onCodeExport={() => setCodeExporterOpen(true)}
                  onClear={() => {
                    setQuery("")
                    runAnalysis("")
                  }}
                  collectionName={collectionName}
                />
              </ResizablePanel>

              <ResizableHandle withHandle />

              <ResizablePanel defaultSize={48} minSize={28}>
                <Inspector
                  analysis={analysis}
                  response={response}
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
                  policyConfig={policyConfig}
                  className="h-full"
                />
              </ResizablePanel>
            </ResizablePanelGroup>
          ) : (
            <div className="flex flex-1 flex-col overflow-hidden">
              <div className="min-h-0 flex-1">
                <EditorPanel
                  ready={ready}
                  query={query}
                  analysis={analysis}
                  executing={executing}
                  settings={settings}
                  browserStatus={browserStatus}
                  onQueryChange={onQueryChange}
                  onExecute={onExecute}
                  onCopyQql={onCopyQql}
                  copiedQql={copiedQql}
                  onCodeExport={() => setCodeExporterOpen(true)}
                  onClear={() => {
                    setQuery("")
                    runAnalysis("")
                  }}
                  collectionName={collectionName}
                />
              </div>
              <div className="flex min-h-0 flex-1 flex-col border-t">
                <div className="flex shrink-0 items-center gap-2 border-b bg-muted/20 px-3 py-1.5">
                  <span className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                    Compiled output
                  </span>
                </div>
                <div className="min-h-0 flex-1">
                  <Inspector
                    analysis={analysis}
                    response={response}
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
                    policyConfig={policyConfig}
                    className="h-full"
                  />
                </div>
              </div>
            </div>
          )}
        </main>

        {/* Compiler Audit Bar */}
        <AuditBar analysis={analysis} query={query} />

        {/* Footer */}
        <footer
          className="flex shrink-0 items-center justify-between gap-2 border-t px-3 py-1 text-[10px] text-muted-foreground"
          aria-label="Status bar"
        >
          <span className="min-w-0 truncate">
            {settings.qdrantUrl}
            {" · "}
            {settings.embedProvider === "browser"
              ? `${BROWSER_EMBED_MODEL}${browserStatus.device ? ` · ${browserStatus.device}` : ""}`
              : settings.embedProvider === "http"
                ? settings.embedModel
                : "no embedder"}
            {" · "}
            {collectionName}
          </span>
          <span className="hidden shrink-0 sm:inline">
            Ctrl/Cmd+Enter execute
          </span>
        </footer>

        {/* Preset capability library dialog */}
        <PresetBrowser
          open={presetBrowserOpen}
          onOpenChange={setPresetBrowserOpen}
          activePresetId={presetId}
          onSelect={onPresetChange}
        />

        {settingsOpen && (
          <SettingsDialog
            open
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
        )}

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
