import { useState, useMemo } from "react"
import { FileTextIcon, TagIcon, SparklesIcon, Code2Icon, LayoutGridIcon, CopyIcon, CheckIcon, LayersIcon } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { JsonViewer } from "@/components/playground/json-viewer"

type ResultCardsProps = {
  responseJson: string
  className?: string
}

export type SearchHit = {
  id: string | number
  score?: number
  payload?: Record<string, unknown>
}

export type GroupedResult = {
  groupKey: string
  hits: SearchHit[]
}

export type ParsedResults = {
  hits: SearchHit[]
  groups: GroupedResult[]
  isGrouped: boolean
  total: number | null
}

function parseSearchHits(rawJson: string): ParsedResults {
  if (!rawJson || rawJson.startsWith("//") || rawJson.startsWith("Executing")) {
    return { hits: [], groups: [], isGrouped: false, total: null }
  }

  try {
    const data = JSON.parse(rawJson)

    // Case 1: Count response
    if (data?.result?.count != null) {
      return { hits: [], groups: [], isGrouped: false, total: data.result.count }
    }

    // Case 2: Grouped response from GROUP BY query
    const rawGroups = data?.result?.groups || data?.result?.result?.groups || (Array.isArray(data?.result) && data?.result[0]?.hits ? data.result : null)

    if (Array.isArray(rawGroups)) {
      const groups: GroupedResult[] = []
      let totalHits = 0

      for (const g of rawGroups) {
        if (g && typeof g === "object" && Array.isArray(g.hits)) {
          const groupKey = String(g.id?.name ?? g.id ?? g.group_key ?? "Group")
          const groupHits: SearchHit[] = []

          for (const item of g.hits) {
            if (item && typeof item === "object") {
              groupHits.push({
                id: item.id ?? "—",
                score: typeof item.score === "number" ? item.score : undefined,
                payload: item.payload && typeof item.payload === "object" ? item.payload : undefined,
              })
              totalHits++
            }
          }

          groups.push({ groupKey, hits: groupHits })
        }
      }

      if (groups.length > 0) {
        return { hits: [], groups, isGrouped: true, total: totalHits }
      }
    }

    // Case 3: Flat array result from search / scroll / query
    let items = data?.result
    if (items && !Array.isArray(items) && Array.isArray(items.points)) {
      items = items.points
    }
    if (items && !Array.isArray(items) && Array.isArray(items.result)) {
      items = items.result
    }

    if (Array.isArray(items)) {
      const hits: SearchHit[] = []
      for (const item of items) {
        if (item && typeof item === "object" && ("id" in item || "score" in item)) {
          hits.push({
            id: item.id ?? "—",
            score: typeof item.score === "number" ? item.score : undefined,
            payload: item.payload && typeof item.payload === "object" ? item.payload : undefined,
          })
        }
      }
      return { hits, groups: [], isGrouped: false, total: hits.length }
    }
  } catch {
    // Fallback if parsing fails
  }

  return { hits: [], groups: [], isGrouped: false, total: null }
}

export function ResultCards({ responseJson, className }: ResultCardsProps) {
  const [viewMode, setViewMode] = useState<"cards" | "json">("cards")
  const [copiedKey, setCopiedKey] = useState<string | null>(null)

  const { hits, groups, isGrouped, total } = useMemo(() => parseSearchHits(responseJson), [responseJson])

  if (!responseJson || responseJson.startsWith("//") || responseJson.startsWith("Executing")) {
    return (
      <JsonViewer
        value={responseJson}
        placeholder="// Execute a query to see the live Qdrant response"
        className={className}
      />
    )
  }

  const hasData = isGrouped ? groups.length > 0 : hits.length > 0

  return (
    <div className={`flex flex-col h-full min-h-0 overflow-hidden ${className ?? ""}`}>
      {/* Header bar with Cards vs Raw JSON toggle */}
      <div className="shrink-0 flex items-center justify-between border-b px-3 py-1.5 bg-muted/20">
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="font-mono text-[10px] gap-1">
            <SparklesIcon className="size-3 text-emerald-500" />
            {isGrouped
              ? `${groups.length} groups (${total ?? 0} total hits)`
              : total != null
                ? `${total} result hits`
                : "Live Qdrant Response"}
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
        {viewMode === "json" || !hasData ? (
          <JsonViewer value={responseJson} className="h-full" />
        ) : isGrouped ? (
          /* Render Grouped Query Result Buckets */
          <div className="flex flex-col gap-4">
            {groups.map((group, gIdx) => (
              <Card key={`group-${gIdx}-${group.groupKey}`} className="border-emerald-500/30 bg-card/60 overflow-hidden">
                <CardHeader className="py-2.5 px-3 bg-muted/30 border-b flex flex-row items-center justify-between">
                  <CardTitle className="text-xs font-mono flex items-center gap-2 font-semibold">
                    <LayersIcon className="size-3.5 text-emerald-500" />
                    <span>Group: <span className="text-emerald-400 font-bold">{group.groupKey}</span></span>
                  </CardTitle>
                  <Badge variant="secondary" className="font-mono text-[10px]">
                    {group.hits.length} hit{group.hits.length > 1 ? "s" : ""}
                  </Badge>
                </CardHeader>
                <CardContent className="p-3 flex flex-col gap-2.5">
                  {group.hits.map((hit, hIdx) => (
                    <HitCard
                      key={`ghit-${group.groupKey}-${hit.id}-${hIdx}`}
                      hit={hit}
                      idx={hIdx}
                      copiedKey={copiedKey}
                      onCopy={(key, text) => {
                        navigator.clipboard.writeText(text)
                        setCopiedKey(key)
                        setTimeout(() => setCopiedKey(null), 2000)
                      }}
                    />
                  ))}
                </CardContent>
              </Card>
            ))}
          </div>
        ) : (
          /* Render Standard Search Hit Cards */
          <div className="flex flex-col gap-3">
            {hits.map((hit, idx) => (
              <HitCard
                key={`hit-${hit.id}-${idx}`}
                hit={hit}
                idx={idx}
                copiedKey={copiedKey}
                onCopy={(key, text) => {
                  navigator.clipboard.writeText(text)
                  setCopiedKey(key)
                  setTimeout(() => setCopiedKey(null), 2000)
                }}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function formatPayloadVal(val: unknown): string {
  if (val == null) return "null"
  if (typeof val === "boolean") return val ? "true" : "false"
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>
    if ("lat" in obj && "lon" in obj) {
      const lat = typeof obj.lat === "number" ? obj.lat.toFixed(4) : String(obj.lat)
      const lon = typeof obj.lon === "number" ? obj.lon.toFixed(4) : String(obj.lon)
      return `📍 ${lat}, ${lon}`
    }
    return JSON.stringify(val)
  }
  return String(val)
}

function HitCard({
  hit,
  idx,
  copiedKey,
  onCopy,
}: {
  hit: SearchHit
  idx: number
  copiedKey: string | null
  onCopy: (key: string, text: string) => void
}) {
  const textContent =
    (hit.payload?.text as string) ||
    (hit.payload?.name as string) ||
    (hit.payload?.document as string) ||
    (hit.payload?.content as string) ||
    null

  const payloadEntries = Object.entries(hit.payload ?? {}).filter(
    ([k]) => k !== "text" && k !== "document" && k !== "content"
  )

  const scorePct = hit.score != null ? Math.min(Math.max(hit.score, 0), 1) * 100 : null
  const copyId = `hit-${hit.id}-${idx}`

  return (
    <Card size="sm" className="overflow-hidden border-border/60 hover:border-primary/40 transition-colors">
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
              onClick={() => onCopy(copyId, JSON.stringify(hit, null, 2))}
              className="font-mono text-[10px] gap-1 h-6 px-1.5"
            >
              {copiedKey === copyId ? <CheckIcon className="size-3 text-emerald-500" /> : <CopyIcon className="size-3" />}
              {copiedKey === copyId ? "Copied" : "Copy Hit"}
            </Button>
          </div>
        </div>

        {/* Text / Name Payload */}
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
            {payloadEntries.slice(0, 12).map(([key, val]) => (
              <Badge key={key} variant="outline" className="font-mono text-[10px] gap-1">
                <span className="text-muted-foreground">{key}:</span>
                <span className="font-semibold text-foreground">{formatPayloadVal(val)}</span>
              </Badge>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
