import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircleIcon,
  CheckCircle2Icon,
  EraserIcon,
  Loader2Icon,
  MoonIcon,
  PlayIcon,
  Settings2Icon,
  SunIcon,
  ZapIcon,
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
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { useTheme } from "@/components/theme-provider"
import { QueryEditor } from "@/components/playground/query-editor"
import { Inspector } from "@/components/playground/inspector"
import { SettingsDialog } from "@/components/playground/settings-dialog"
import { useQql } from "@/hooks/use-qql"
import {
  DEFAULT_PRESET_ID,
  PRESETS,
  getPreset,
  type PresetId,
} from "@/lib/presets"

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

export function App() {
  const { theme, setTheme } = useTheme()
  const {
    ready,
    initError,
    settings,
    updateSettings,
    analysis,
    latencyMs,
    response,
    executing,
    runAnalysis,
    execute,
  } = useQql()

  const [presetId, setPresetId] = useState<PresetId>(DEFAULT_PRESET_ID)
  const [query, setQuery] = useState(
    () => getPreset(DEFAULT_PRESET_ID)?.query ?? ""
  )
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [activeTab, setActiveTab] = useState("plan")

  const debouncedAnalyze = useDebouncedCallback((src: string) => {
    runAnalysis(src)
  }, 80)

  useEffect(() => {
    if (ready) runAnalysis(query)
  }, [ready]) // eslint-disable-line react-hooks/exhaustive-deps

  const onQueryChange = (value: string) => {
    setQuery(value)
    debouncedAnalyze(value)
  }

  const onPresetChange = (id: string | null) => {
    if (!id) return
    const preset = getPreset(id)
    if (!preset) return
    setPresetId(preset.id)
    setQuery(preset.query)
    runAnalysis(preset.query)
  }

  const onExecute = async () => {
    setActiveTab("response")
    await execute(query)
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
        {/* Toolbar */}
        <header className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-2 sm:gap-3 sm:px-4">
          <div className="flex items-center gap-2">
            <span className="text-base font-semibold tracking-tight">QQL</span>
            <Badge variant="secondary" className="gap-1 font-mono text-[10px]">
              <ZapIcon className="size-3" />
              WASM
            </Badge>
          </div>

          <Separator orientation="vertical" className="hidden h-6 sm:block" />

          <div className="flex min-w-0 flex-1 items-center gap-2">
            <span className="hidden text-xs text-muted-foreground sm:inline">
              Preset
            </span>
            <Select value={presetId} onValueChange={onPresetChange}>
              <SelectTrigger size="sm" className="max-w-[min(100%,280px)]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PRESETS.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
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

            <span className="hidden font-mono text-[11px] text-muted-foreground tabular-nums md:inline">
              {latencyMs.toFixed(2)} ms
            </span>

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
                  <Button variant="outline" size="icon-sm" onClick={toggleTheme} />
                }
              >
                {theme === "dark" ? <SunIcon /> : <MoonIcon />}
              </TooltipTrigger>
              <TooltipContent>Toggle theme</TooltipContent>
            </Tooltip>

            <Button
              size="sm"
              disabled={!ready || !analysis.valid || executing}
              onClick={onExecute}
            >
              {executing ? (
                <Loader2Icon className="animate-spin" data-icon="inline-start" />
              ) : (
                <PlayIcon data-icon="inline-start" />
              )}
              Execute
            </Button>
          </div>
        </header>

        {/* Workspace */}
        <main className="min-h-0 flex-1">
          <ResizablePanelGroup orientation="horizontal" className="h-full">
            <ResizablePanel defaultSize={52} minSize={30}>
              <section className="flex h-full min-h-0 flex-col">
                <div className="flex shrink-0 items-center justify-between border-b px-3 py-1.5">
                  <span className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                    Query editor
                  </span>
                  <div className="flex gap-1">
                    <Button
                      variant="ghost"
                      size="xs"
                      onClick={() => {
                        setQuery("")
                        runAnalysis("")
                      }}
                    >
                      <EraserIcon data-icon="inline-start" />
                      Clear
                    </Button>
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
                      Loading qql-wasm…
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
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel defaultSize={48} minSize={28}>
              <Inspector
                analysis={analysis}
                response={response}
                activeTab={activeTab}
                onTabChange={setActiveTab}
                className="h-full"
              />
            </ResizablePanel>
          </ResizablePanelGroup>
        </main>

        <footer className="flex shrink-0 items-center justify-between border-t px-3 py-1.5 text-[11px] text-muted-foreground">
          <span>
            {settings.qdrantUrl}
            {settings.embedProvider !== "none"
              ? ` · ${settings.embedModel}`
              : " · no embedder"}
          </span>
          <span className="hidden sm:inline">
            ⌘/Ctrl+Enter execute · d toggles theme
          </span>
        </footer>

        <SettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          settings={settings}
          onSave={updateSettings}
        />
      </div>
    </TooltipProvider>
  )
}

export default App
