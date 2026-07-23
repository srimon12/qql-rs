import { useState, useMemo } from "react"
import { FileTextIcon, TagIcon, SparklesIcon, Code2Icon, LayoutGridIcon, CopyIcon, CheckIcon } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { JsonViewer } from "@/components/playground/json-viewer"

type ResultCardsProps = {
  responseJson: string
  className?: string
}

type SearchHit = {
  id: string | number
  score?: number
  payload?: Record<string, unknown>
}

function parseSearchHits(rawJson: string): { hits: SearchHit[]; isGrouped: boolean; total: number | null } {
  if (!rawJson || rawJson.startsWith("//") || rawJson.startsWith("Executing")) {
    return { hits: [], isGrouped: false, total: null }
  }

  try {
    const data = JSON.parse(rawJson)

    // Case 1: Count response
    if (data?.result?.count != null) {
      return { hits: [], isGrouped: false, total: data.result.count }
    }

    // Case 2: Array result from search / scroll / query
    let items = data?.result
    if (items && !Array.isArray(items) && Array.isArray(items.points)) {
      items = items.points
    }

    if (Array.isArray(items)) {
      const hits: SearchHit[] = []
      for (const item of items) {
        if (item && typeof item === "object") {
          // Check standard search point
          if ("id" in item || "score" in item) {
            hits.push({
              id: item.id ?? "—",
              score: typeof item.score === "number" ? item.score : undefined,
              payload: item.payload && typeof item.payload === "object" ? item.payload : undefined,
            })
          }
          // Check grouped result
          else if (Array.isArray((item as Record<string, unknown>).hits)) {
            const groupHits = (item as Record<string, unknown>).hits as SearchHit[]
            for (const gh of groupHits) {
              hits.push({
                id: gh.id ?? "—",
                score: typeof gh.score === "number" ? gh.score : undefined,
                payload: gh.payload && typeof gh.payload === "object" ? gh.payload : undefined,
              })
            }
          }
        }
      }
      return { hits, isGrouped: false, total: hits.length }
    }
  } catch {
    // Fallback if parsing fails
  }

  return { hits: [], isGrouped: false, total: null }
}

export function ResultCards({ responseJson, className }: ResultCardsProps) {
  const [viewMode, setViewMode] = useState<"cards" | "json">("cards")
  const [copiedIdx, setCopiedIdx] = useState<number | null>(null)

  const { hits, total } = useMemo(() => parseSearchHits(responseJson), [responseJson])

  if (!responseJson || responseJson.startsWith("//") || responseJson.startsWith("Executing")) {
    return (
      <JsonViewer
        value={responseJson}
        placeholder="// Execute a query to see the live Qdrant response"
        className={className}
      />
    )
  }

  return (
    <div className={`flex flex-col h-full min-h-0 overflow-hidden ${className ?? ""}`}>
      {/* Header bar with Cards vs Raw JSON toggle */}
      <div className="shrink-0 flex items-center justify-between border-b px-3 py-1.5 bg-muted/20">
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="font-mono text-[10px] gap-1">
            <SparklesIcon className="size-3 text-emerald-500" />
            {total != null ? `${total} result hits` : "Live Qdrant Response"}
          </Badge>
        </div>

        <div className="flex items-center gap-1 border rounded-lg p-0.5 bg-muted/40">
          <Button
            variant={viewMode === "cards" ? "secondary" : "ghost"}
            size="xs"
            onClick={() => setViewMode("cards")}
            className="gap-1 font-mono text-[10px]"
          >
            <LayoutGridIcon className="size-3" />
            Result Cards
          </Button>
          <Button
            variant={viewMode === "json" ? "secondary" : "ghost"}
            size="xs"
            onClick={() => setViewMode("json")}
            className="gap-1 font-mono text-[10px]"
          >
            <Code2Icon className="size-3" />
            Raw JSON
          </Button>
        </div>
      </div>

      {/* Main View */}
      <div className="flex-1 min-h-0 overflow-auto p-3">
        {viewMode === "json" || hits.length === 0 ? (
          <JsonViewer value={responseJson} className="h-full" />
        ) : (
          <div className="flex flex-col gap-3">
            {hits.map((hit, idx) => {
              const textContent =
                (hit.payload?.text as string) ||
                (hit.payload?.document as string) ||
                (hit.payload?.content as string) ||
                null

              const payloadEntries = Object.entries(hit.payload ?? {}).filter(
                ([k]) => k !== "text" && k !== "document" && k !== "content"
              )

              const scorePct = hit.score != null ? Math.min(Math.max(hit.score, 0), 1) * 100 : null

              return (
                <Card key={`${hit.id}-${idx}`} size="sm" className="overflow-hidden border-border/60 hover:border-primary/40 transition-colors">
                  <CardContent className="p-3 flex flex-col gap-2">
                    <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2">
                      <div className="flex items-center gap-2">
                        <Badge variant="default" className="font-mono text-[10px] bg-primary/80">
                          #{idx + 1}
                        </Badge>
                        <span className="font-mono text-xs text-muted-foreground">
                          ID: <span className="text-foreground font-semibold">{String(hit.id)}</span>
                        </span>
                      </div>

                      <div className="flex items-center gap-3">
                        {hit.score != null && (
                          <div className="flex items-center gap-2">
                            <span className="font-mono text-xs font-semibold tabular-nums text-emerald-600 dark:text-emerald-400">
                              Score {hit.score.toFixed(4)}
                            </span>
                            {scorePct != null && (
                              <div className="w-16 h-1.5 rounded-full bg-muted overflow-hidden">
                                <div
                                  className="h-full bg-emerald-500 rounded-full"
                                  style={{ width: `${scorePct}%` }}
                                />
                              </div>
                            )}
                          </div>
                        )}

                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={() => {
                            navigator.clipboard.writeText(JSON.stringify(hit, null, 2))
                            setCopiedIdx(idx)
                            setTimeout(() => setCopiedIdx(null), 2000)
                          }}
                          className="font-mono text-[10px] gap-1 h-6 px-1.5"
                        >
                          {copiedIdx === idx ? <CheckIcon className="size-3 text-emerald-500" /> : <CopyIcon className="size-3" />}
                          {copiedIdx === idx ? "Copied" : "Copy Hit"}
                        </Button>
                      </div>
                    </div>

                    {/* Text Payload */}
                    {textContent ? (
                      <div className="flex items-start gap-2 text-xs leading-relaxed text-foreground/90 font-mono bg-muted/20 p-2.5 rounded-md border">
                        <FileTextIcon className="size-4 shrink-0 mt-0.5 text-muted-foreground" />
                        <span className="line-clamp-4">{textContent}</span>
                      </div>
                    ) : (
                      <div className="text-xs text-muted-foreground italic">No text payload field previewable</div>
                    )}

                    {/* Metadata Payload Chips */}
                    {payloadEntries.length > 0 && (
                      <div className="flex flex-wrap items-center gap-1.5 pt-1">
                        <TagIcon className="size-3 text-muted-foreground" />
                        {payloadEntries.slice(0, 8).map(([key, val]) => (
                          <Badge key={key} variant="outline" className="font-mono text-[10px] gap-1">
                            <span className="text-muted-foreground">{key}:</span>
                            <span className="font-semibold text-foreground">{String(val)}</span>
                          </Badge>
                        ))}
                      </div>
                    )}
                  </CardContent>
                </Card>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
