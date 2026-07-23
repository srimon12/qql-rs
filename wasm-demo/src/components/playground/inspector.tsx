import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ScrollArea } from "@/components/ui/scroll-area"
import { PlanView } from "@/components/playground/plan-view"
import { TokensTable } from "@/components/playground/tokens-table"
import { JsonViewer } from "@/components/playground/json-viewer"
import type { AnalysisResult } from "@/lib/qql-types"
import { cn } from "@/lib/utils"

type InspectorProps = {
  analysis: AnalysisResult
  response: string
  activeTab: string
  onTabChange: (tab: string) => void
  className?: string
}

export function Inspector({
  analysis,
  response,
  activeTab,
  onTabChange,
  className,
}: InspectorProps) {
  const wireJson = analysis.route
    ? JSON.stringify(analysis.route.payload ?? null, null, 2)
    : analysis.error
      ? JSON.stringify(analysis.error, null, 2)
      : "{}"

  const astJson = analysis.ast
    ? JSON.stringify(analysis.ast, null, 2)
    : analysis.error
      ? JSON.stringify(analysis.error, null, 2)
      : "{}"

  return (
    <Tabs
      value={activeTab}
      onValueChange={onTabChange}
      className={cn("flex h-full min-h-0 flex-col gap-0", className)}
    >
      <div className="shrink-0 border-b px-3 pt-2 pb-0">
        <TabsList variant="line" className="h-auto w-full flex-wrap justify-start gap-0">
          <TabsTrigger value="plan">Plan</TabsTrigger>
          <TabsTrigger value="wire">Wire JSON</TabsTrigger>
          <TabsTrigger value="ast">AST</TabsTrigger>
          <TabsTrigger value="tokens">Tokens</TabsTrigger>
          <TabsTrigger value="explain">Explain</TabsTrigger>
          <TabsTrigger value="response">Response</TabsTrigger>
        </TabsList>
      </div>

      <TabsContent value="plan" className="min-h-0 overflow-auto p-3">
        <PlanView analysis={analysis} />
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
        <JsonViewer
          value={response}
          placeholder="// Execute a query to see the live Qdrant response"
          className="h-full"
        />
      </TabsContent>
    </Tabs>
  )
}
