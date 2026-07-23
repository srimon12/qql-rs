import { ShieldCheckIcon, ShieldAlertIcon, LayersIcon, DatabaseIcon, CheckCircle2Icon, AlertCircleIcon } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { AnalysisResult } from "@/lib/qql-types"

type AuditBarProps = {
  analysis: AnalysisResult
  query: string
  className?: string
}

function extractShardKey(query: string, ast: unknown): string | null {
  // Check regex pattern SHARD '...' or SHARD "..."
  const match = query.match(/SHARD\s+['"]([^'"]+)['"]/i)
  if (match) return match[1]

  // Check AST if available
  if (Array.isArray(ast)) {
    for (const stmt of ast) {
      if (stmt && typeof stmt === "object") {
        for (const key of Object.keys(stmt)) {
          const val = (stmt as Record<string, unknown>)[key]
          if (val && typeof val === "object" && "shard_key" in val) {
            const sk = (val as { shard_key?: string }).shard_key
            if (sk) return sk
          }
        }
      }
    }
  }
  return null
}

function hasFilter(query: string, ast: unknown): boolean {
  if (/WHERE\b/i.test(query)) return true
  if (Array.isArray(ast)) {
    const jsonStr = JSON.stringify(ast)
    return jsonStr.includes('"filter":') && !jsonStr.includes('"filter":null')
  }
  return false
}

export function AuditBar({ analysis, query, className }: AuditBarProps) {
  if (!analysis.valid && !query.trim()) return null

  const shardKey = extractShardKey(query, analysis.ast)
  const isFiltered = hasFilter(query, analysis.ast)
  const stmtCount = analysis.statements_count

  return (
    <div className={`flex flex-wrap items-center gap-2 border-b bg-muted/30 px-3 py-1.5 text-xs ${className ?? ""}`}>
      <span className="font-mono text-[10px] uppercase tracking-wider text-muted-foreground font-medium">
        Compiler Audit:
      </span>

      {/* Validity */}
      <Badge variant={analysis.valid ? "outline" : "destructive"} className="gap-1 font-mono text-[10px]">
        {analysis.valid ? (
          <CheckCircle2Icon className="size-3 text-emerald-500" />
        ) : (
          <AlertCircleIcon className="size-3" />
        )}
        {analysis.valid ? "AST Valid" : "Syntax Error"}
      </Badge>

      {/* Filter Present Security Check */}
      <Tooltip>
        <TooltipTrigger render={
          <Badge variant={isFiltered ? "secondary" : "outline"} className="gap-1 font-mono text-[10px]">
            {isFiltered ? (
              <ShieldCheckIcon className="size-3 text-emerald-500" />
            ) : (
              <ShieldAlertIcon className="size-3 text-amber-500" />
            )}
            {isFiltered ? "Filter Present" : "Unfiltered"}
          </Badge>
        } />
        <TooltipContent>
          {isFiltered
            ? "Query contains payload filter constraints (WHERE clause or injected filter)."
            : "Caution: Query is unfiltered and may access full collection scope."}
        </TooltipContent>
      </Tooltip>

      {/* Physical Shard Target */}
      <Tooltip>
        <TooltipTrigger render={
          <Badge variant={shardKey ? "default" : "outline"} className="gap-1 font-mono text-[10px]">
            <DatabaseIcon className="size-3" />
            {shardKey ? `Shard: ${shardKey}` : "All Shards"}
          </Badge>
        } />
        <TooltipContent>
          {shardKey
            ? `Physical shard routing active (targets custom shard '${shardKey}').`
            : "No custom shard routing set (queries default cluster sharding)."}
        </TooltipContent>
      </Tooltip>

      {/* Statement Count */}
      {stmtCount > 0 && (
        <Badge variant="outline" className="gap-1 font-mono text-[10px]">
          <LayersIcon className="size-3 text-muted-foreground" />
          {stmtCount} {stmtCount === 1 ? "statement" : "statements"}
        </Badge>
      )}
    </div>
  )
}
