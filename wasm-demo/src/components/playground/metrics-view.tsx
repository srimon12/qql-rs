import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import type { ExecMetrics } from "@/lib/qql-types"
import type { BrowserEmbedderStatus } from "@/lib/browser-embedder"
import { BROWSER_EMBED_MODEL, BROWSER_EMBED_DIM } from "@/lib/browser-embedder"
import { cn } from "@/lib/utils"

type MetricsViewProps = {
  metrics: ExecMetrics | null
  parseMs: number
  browserStatus: BrowserEmbedderStatus
  embedProvider: string
  qdrantUrl: string
}

function MetricCell({
  label,
  value,
  hint,
  accent,
}: {
  label: string
  value: string
  hint?: string
  accent?: boolean
}) {
  return (
    <div
      className={cn(
        "rounded-xl border bg-card p-3",
        accent && "border-primary/30 bg-primary/5"
      )}
    >
      <div className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
        {label}
      </div>
      <div className="mt-1 font-mono text-lg font-semibold tabular-nums tracking-tight">
        {value}
      </div>
      {hint ? (
        <div className="mt-0.5 text-[11px] text-muted-foreground">{hint}</div>
      ) : null}
    </div>
  )
}

function fmtMs(ms: number | null | undefined, digits = 2): string {
  if (ms == null || Number.isNaN(ms)) return "—"
  if (ms < 1) return `${(ms * 1000).toFixed(0)} µs`
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)} s`
  return `${ms.toFixed(digits)} ms`
}

export function MetricsView({
  metrics,
  parseMs,
  browserStatus,
  embedProvider,
  qdrantUrl,
}: MetricsViewProps) {
  return (
    <div className="flex flex-col gap-4 p-3">
      <Card size="sm">
        <CardHeader className="pb-0">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <CardTitle className="text-sm">Embedder</CardTitle>
            <Badge
              variant={
                browserStatus.state === "ready"
                  ? "default"
                  : browserStatus.state === "error"
                    ? "destructive"
                    : "secondary"
              }
            >
              {embedProvider === "browser"
                ? browserStatus.state
                : embedProvider}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="grid gap-2 text-xs">
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">Model</span>
            <span className="font-mono text-right break-all">
              {embedProvider === "browser"
                ? BROWSER_EMBED_MODEL
                : embedProvider === "http"
                  ? "HTTP OpenAI-compatible"
                  : "None"}
            </span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">Dimension</span>
            <span className="font-mono">
              {embedProvider === "browser"
                ? BROWSER_EMBED_DIM
                : metrics?.embedDim ?? 384}
            </span>
          </div>
          {embedProvider === "browser" && (
            <>
              <div className="flex justify-between gap-4">
                <span className="text-muted-foreground">Device</span>
                <span className="font-mono">
                  {browserStatus.device ?? "—"}
                </span>
              </div>
              <div className="flex justify-between gap-4">
                <span className="text-muted-foreground">Model load</span>
                <span className="font-mono">
                  {fmtMs(browserStatus.loadMs)}
                </span>
              </div>
              <div className="flex justify-between gap-4">
                <span className="text-muted-foreground">Status</span>
                <span className="text-right">{browserStatus.statusText}</span>
              </div>
              {browserStatus.state === "loading" && (
                <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full bg-primary transition-all"
                    style={{ width: `${browserStatus.progress}%` }}
                  />
                </div>
              )}
              {browserStatus.error && (
                <p className="text-destructive">{browserStatus.error}</p>
              )}
            </>
          )}
          <Separator className="my-1" />
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">Qdrant</span>
            <span className="font-mono text-right break-all">{qdrantUrl}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">Collection showcase</span>
            <span className="font-mono">sec10k (sharded tenants)</span>
          </div>
        </CardContent>
      </Card>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        <MetricCell
          label="Parse / plan"
          value={fmtMs(metrics?.parseMs ?? parseMs)}
          hint="analyze() in WASM"
        />
        <MetricCell
          label="Embed"
          value={fmtMs(metrics?.embedMs)}
          hint={
            metrics?.embedTexts
              ? `${metrics.embedTexts} text(s) · ${metrics.embedDim}-d`
              : "no embed this run"
          }
          accent
        />
        <MetricCell
          label="Network / Qdrant"
          value={fmtMs(metrics?.networkMs)}
          hint="approx total − embed"
        />
        <MetricCell
          label="Total execute"
          value={fmtMs(metrics?.totalMs)}
          hint="client.execute wall"
          accent
        />
        <MetricCell
          label="Statements"
          value={
            metrics
              ? String(metrics.statements)
              : "—"
          }
        />
        <MetricCell
          label="Backend"
          value={metrics?.embedBackend ?? (embedProvider === "browser" ? (browserStatus.device ?? "…") : embedProvider)}
          hint={metrics?.embedModel}
        />
      </div>

      {!metrics && (
        <p className="text-center text-xs text-muted-foreground">
          Run <strong>Execute</strong> to capture embed + Qdrant timings.
          Parse latency updates live as you type.
        </p>
      )}

      {metrics && !metrics.success && (
        <p className="rounded-xl border border-destructive/30 bg-destructive/5 p-3 font-mono text-xs text-destructive">
          {metrics.error}
        </p>
      )}

      {metrics?.success && metrics.at > 0 && (
        <p className="text-[11px] text-muted-foreground">
          Last run {new Date(metrics.at).toLocaleTimeString()}
          {metrics.embedMs != null && metrics.embedTexts > 0
            ? ` · ${(metrics.embedMs / metrics.embedTexts).toFixed(1)} ms/text`
            : ""}
        </p>
      )}
    </div>
  )
}
