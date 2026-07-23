import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ScrollArea } from "@/components/ui/scroll-area"
import { PlanView } from "@/components/playground/plan-view"
import { TokensTable } from "@/components/playground/tokens-table"
import { ShieldCheckIcon } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { JsonViewer } from "@/components/playground/json-viewer"
import { MetricsView } from "@/components/playground/metrics-view"
import { ResultCards } from "@/components/playground/results-cards"
import type { AnalysisResult, ExecMetrics, InspectorTab, EmbedProvider, TenantConfig } from "@/lib/qql-types"
import type { BrowserEmbedderStatus } from "@/lib/browser-embedder"
import { cn } from "@/lib/utils"

type InspectorProps = {
  analysis: AnalysisResult
  responseJson: string
  activeTab: InspectorTab
  onTabChange: (tab: InspectorTab) => void
  metrics: ExecMetrics | null
  parseMs: number
  browserStatus: BrowserEmbedderStatus
  embedProvider: EmbedProvider
  qdrantUrl: string
  teachingNote?: string
  selectedStmtIndex?: number
  onSelectStmtIndex?: (idx: number) => void
  tenantConfig?: TenantConfig
  className?: string
}

export function Inspector({
  analysis,
  responseJson,
  activeTab,
  onTabChange,
  metrics,
  parseMs,
  browserStatus,
  embedProvider,
  qdrantUrl,
  teachingNote,
  selectedStmtIndex = 0,
  onSelectStmtIndex,
  tenantConfig,
  className,
}: InspectorProps) {
  const setSelectedStmtIndex = onSelectStmtIndex ?? (() => {})

  const routes = analysis.routes && analysis.routes.length > 0
    ? analysis.routes
    : analysis.route ? [analysis.route] : []

  const currentRoute = routes[selectedStmtIndex] ?? routes[0] ?? analysis.route

  const wireJson = currentRoute
    ? JSON.stringify(currentRoute.payload ?? null, null, 2)
    : analysis.error
      ? JSON.stringify(analysis.error, null, 2)
      : "{}"

  let astJson = "{}"
  if (Array.isArray(analysis.ast)) {
    const currentAst = analysis.ast[selectedStmtIndex] ?? analysis.ast[0] ?? analysis.ast
    astJson = JSON.stringify(currentAst, null, 2)
  } else if (analysis.ast) {
    astJson = JSON.stringify(analysis.ast, null, 2)
  } else if (analysis.error) {
    astJson = JSON.stringify(analysis.error, null, 2)
  }

  return (
    <Tabs
      value={activeTab}
      onValueChange={onTabChange}
      className={cn("flex h-full min-h-0 flex-col gap-0", className)}
    >
      <div className="shrink-0 border-b px-3 pt-2 pb-0 flex items-center justify-between">
        <TabsList
          variant="line"
          className="h-auto w-full flex-wrap justify-start gap-0"
        >
          <TabsTrigger value="plan">Plan</TabsTrigger>
          <TabsTrigger value="metrics">Metrics</TabsTrigger>
          <TabsTrigger value="wire">Wire JSON</TabsTrigger>
          <TabsTrigger value="ast">AST</TabsTrigger>
          <TabsTrigger value="tokens">Tokens</TabsTrigger>
          <TabsTrigger value="explain">Explain</TabsTrigger>
          <TabsTrigger value="response">Response</TabsTrigger>
        </TabsList>

        {tenantConfig?.enabled && (
          <Badge variant="outline" className="font-mono text-[10px] bg-emerald-500/10 border-emerald-500/40 text-emerald-400 gap-1 shrink-0 ml-2">
            <ShieldCheckIcon className="size-3 text-emerald-500" />
            <span>AST Injected</span>
          </Badge>
        )}
      </div>

      <TabsContent value="plan" className="min-h-0 overflow-auto p-3">
        <PlanView
          analysis={analysis}
          selectedStmtIndex={selectedStmtIndex}
          onSelectStmtIndex={setSelectedStmtIndex}
          teachingNote={teachingNote}
        />
      </TabsContent>

      <TabsContent value="metrics" className="min-h-0 overflow-auto p-0">
        <ScrollArea className="h-full">
          <MetricsView
            metrics={metrics}
            parseMs={parseMs}
            browserStatus={browserStatus}
            embedProvider={embedProvider}
            qdrantUrl={qdrantUrl}
          />
        </ScrollArea>
      </TabsContent>

      <TabsContent value="wire" className="min-h-0 overflow-hidden p-0">
        <JsonViewer value={wireJson} className="h-full" />
      </TabsContent>

      <TabsContent value="ast" className="min-h-0 overflow-hidden p-0">
        <JsonViewer value={astJson} className="h-full" />
      </TabsContent>

      <TabsContent value="tokens" className="min-h-0 overflow-auto p-0">
        <ScrollArea className="h-full">
          <TokensTable tokens={analysis.tokens} />
        </ScrollArea>
      </TabsContent>

      <TabsContent value="explain" className="min-h-0 overflow-auto p-4">
        <pre className="font-mono text-xs leading-relaxed whitespace-pre-wrap">
          {analysis.explain ||
            analysis.error?.message ||
            "No explanation available."}
        </pre>
      </TabsContent>

      <TabsContent value="response" className="min-h-0 overflow-hidden p-0">
        <ResultCards responseJson={responseJson} className="h-full" />
      </TabsContent>
    </Tabs>
  )
}
