import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import type { AnalysisResult } from "@/lib/qql-types"
import { LayersIcon, LightbulbIcon } from "lucide-react"

type PlanViewProps = {
  analysis: AnalysisResult
  selectedStmtIndex: number
  onSelectStmtIndex: (index: number) => void
  teachingNote?: string
}

function methodVariant(
  method?: string
): "default" | "secondary" | "destructive" | "outline" {
  switch ((method ?? "").toUpperCase()) {
    case "POST":
    case "PUT":
    case "PATCH":
      return "default"
    case "DELETE":
      return "destructive"
    case "GET":
      return "secondary"
    default:
      return "outline"
  }
}

export function PlanView({
  analysis,
  selectedStmtIndex,
  onSelectStmtIndex,
  teachingNote,
}: PlanViewProps) {
  const routes = analysis.routes && analysis.routes.length > 0
    ? analysis.routes
    : analysis.route ? [analysis.route] : []

  const currentRoute = routes[selectedStmtIndex] ?? routes[0] ?? analysis.route
  const method = currentRoute?.method?.toUpperCase() ?? "—"
  const path = currentRoute?.path ?? "—"

  return (
    <div className="flex flex-col gap-4 p-1">
      {/* Preset / Concept Teaching Note */}
      {teachingNote && (
        <Card size="sm" className="border-sky-500/30 bg-sky-500/5">
          <CardContent className="p-3 flex items-start gap-2 text-xs">
            <LightbulbIcon className="size-4 text-sky-500 shrink-0 mt-0.5" />
            <div>
              <span className="font-semibold text-sky-600 dark:text-sky-400 block mb-0.5">
                What this statement teaches
              </span>
              <p className="text-muted-foreground leading-relaxed">{teachingNote}</p>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Multi-statement selector */}
      {analysis.statements_count > 1 && (
        <div className="flex items-center gap-2 border rounded-lg p-2 bg-muted/20">
          <LayersIcon className="size-4 text-muted-foreground shrink-0" />
          <span className="text-xs font-mono font-medium text-muted-foreground">
            Multi-Statement Script ({analysis.statements_count}):
          </span>
          <div className="flex flex-wrap gap-1">
            {Array.from({ length: analysis.statements_count }).map((_, idx) => (
              <Button
                key={idx}
                variant={selectedStmtIndex === idx ? "default" : "outline"}
                size="xs"
                onClick={() => onSelectStmtIndex(idx)}
                className="font-mono text-[11px]"
              >
                Statement #{idx + 1}
              </Button>
            ))}
          </div>
        </div>
      )}

      <Card size="sm" className="ring-foreground/5">
        <CardHeader className="pb-0">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm">REST route projection</CardTitle>
            {analysis.statements_count > 1 && (
              <Badge variant="secondary" className="font-mono text-[10px]">
                Stmt {selectedStmtIndex + 1} of {analysis.statements_count}
              </Badge>
            )}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-3 pt-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={methodVariant(method)} className="font-mono">
              {method}
            </Badge>
            <code className="font-mono text-sm break-all text-primary">
              {path}
            </code>
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {analysis.error
              ? analysis.error.message
              : analysis.statements_count > 1
                ? `Showing compiled REST route for statement #${selectedStmtIndex + 1}.`
                : analysis.valid
                  ? "Compiled QQL statement projected to the Qdrant REST handler."
                  : "No route generated yet."}
          </p>
        </CardContent>
      </Card>

      <Card size="sm" className="ring-foreground/5">
        <CardHeader className="pb-0">
          <CardTitle className="text-sm">Plan explanation</CardTitle>
        </CardHeader>
        <CardContent className="pt-3">
          <pre className="font-mono text-xs leading-relaxed whitespace-pre-wrap text-foreground/90">
            {analysis.explain ||
              analysis.error?.message ||
              "No plan explanation available."}
          </pre>
        </CardContent>
      </Card>
    </div>
  )
}
