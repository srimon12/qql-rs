import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import type { AnalysisResult } from "@/lib/qql-types"

type PlanViewProps = {
  analysis: AnalysisResult
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

export function PlanView({ analysis }: PlanViewProps) {
  const method = analysis.route?.method?.toUpperCase() ?? "—"
  const path = analysis.route?.path ?? "—"

  return (
    <div className="flex flex-col gap-4 p-1">
      <Card size="sm" className="ring-foreground/5">
        <CardHeader className="pb-0">
          <CardTitle className="text-sm">REST route</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
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
                ? `Script contains ${analysis.statements_count} statements. Showing the first compiled route.`
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
        <CardContent>
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
