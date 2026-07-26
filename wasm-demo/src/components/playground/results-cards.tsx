import { useState, useMemo } from "react"
import {
  FileTextIcon,
  TagIcon,
  SparklesIcon,
  Code2Icon,
  LayoutGridIcon,
  CopyIcon,
  CheckIcon,
  LayersIcon,
} from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { JsonViewer } from "@/components/playground/json-viewer"

type ResultCardsProps = {
  response: unknown
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
  kind: "hits" | "groups" | "count" | "none"
  total: number | null
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null
}

function asHit(value: unknown): SearchHit | null {
  const hit = asRecord(value)
  if (!hit || (!("id" in hit) && !("score" in hit))) return null
  return {
    id: typeof hit.id === "string" || typeof hit.id === "number" ? hit.id : "—",
    score: typeof hit.score === "number" ? hit.score : undefined,
    payload: asRecord(hit.payload) ?? undefined,
  }
}

type ReportDatum = {
  operation: string | null
  data: unknown
}

function reportData(raw: unknown): ReportDatum[] {
  const report = asRecord(raw)
  if (!report || !Array.isArray(report.results)) {
    return [{ operation: null, data: raw }]
  }

  return report.results.flatMap((result) => {
    const response = asRecord(result)
    if (!response || response.data == null) return []
    return [
      {
        operation:
          typeof response.operation === "string" ? response.operation : null,
        data: response.data,
      },
    ]
  })
}

function groupKey(value: unknown): string {
  const objectKey = asRecord(value)
  if (objectKey && objectKey.name != null) return String(objectKey.name)
  return value == null ? "Group" : String(value)
}

function groupsFrom(data: unknown): GroupedResult[] {
  const root = asRecord(data)
  const result = asRecord(root?.result)
  const rawGroups = root?.groups ?? result?.groups
  if (!Array.isArray(rawGroups)) return []

  return rawGroups.flatMap((rawGroup) => {
    const group = asRecord(rawGroup)
    if (!group || !Array.isArray(group.hits)) return []
    const hits = group.hits.flatMap((hit) => {
      const parsed = asHit(hit)
      return parsed ? [parsed] : []
    })
    return [
      {
        groupKey: groupKey(group.id ?? group.group_key),
        hits,
      },
    ]
  })
}

function hitsFrom(data: unknown): SearchHit[] {
  const root = asRecord(data)
  let items: unknown = root?.result ?? data
  const result = asRecord(items)
  if (result) {
    if (Array.isArray(result.points)) items = result.points
    else if (Array.isArray(result.result)) items = result.result
  }

  if (!Array.isArray(items)) return []
  return items.flatMap((item) => {
    const hit = asHit(item)
    return hit ? [hit] : []
  })
}

function countFrom(data: unknown): number | null {
  const root = asRecord(data)
  const result = asRecord(root?.result)
  const count = result?.count ?? root?.count
  return typeof count === "number" ? count : null
}

function parseSearchHits(raw: unknown): ParsedResults {
  if (raw == null || raw === "") {
    return { hits: [], groups: [], kind: "none", total: null }
  }

  const hits: SearchHit[] = []
  const groups: GroupedResult[] = []
  let countTotal = 0
  let hasCount = false
  let hasGroups = false
  let hasHits = false

  for (const { operation, data } of reportData(raw)) {
    const parsedGroups = groupsFrom(data)
    if (operation === "QUERY_GROUPS" || parsedGroups.length > 0) {
      hasGroups = true
      groups.push(...parsedGroups)
      continue
    }

    const count = countFrom(data)
    if (operation === "COUNT" || count != null) {
      hasCount = true
      countTotal += count ?? 0
      continue
    }

    const parsedHits = hitsFrom(data)
    if (
      operation === "QUERY" ||
      operation === "SCROLL" ||
      operation === "GET_POINTS" ||
      parsedHits.length > 0 ||
      Array.isArray(data)
    ) {
      hasHits = true
      hits.push(...parsedHits)
    }
  }

  const resultKinds = Number(hasGroups) + Number(hasHits) + Number(hasCount)
  if (resultKinds > 1) {
    return { hits: [], groups: [], kind: "none", total: null }
  }
  if (hasGroups) {
    return {
      hits: [],
      groups,
      kind: "groups",
      total: groups.reduce((sum, group) => sum + group.hits.length, 0),
    }
  }
  if (hasHits) {
    return { hits, groups: [], kind: "hits", total: hits.length }
  }
  if (hasCount) {
    return { hits: [], groups: [], kind: "count", total: countTotal }
  }
  return { hits: [], groups: [], kind: "none", total: null }
}

export function ResultCards({ response, className }: ResultCardsProps) {
  const [viewMode, setViewMode] = useState<"cards" | "json">("cards")
  const [copiedKey, setCopiedKey] = useState<string | null>(null)

  const { hits, groups, kind, total } = useMemo(
    () => parseSearchHits(response),
    [response]
  )

  const responseStr =
    typeof response === "string"
      ? response
      : (JSON.stringify(response, null, 2) ?? String(response ?? ""))

  if (
    !response ||
    (typeof response === "string" &&
      (response.startsWith("//") || response.startsWith("Executing")))
  ) {
    return (
      <JsonViewer
        value={responseStr}
        placeholder="// Execute a query to see the live Qdrant response"
        className={className}
      />
    )
  }

  return (
    <div
      className={`flex h-full min-h-0 flex-col overflow-hidden ${className ?? ""}`}
    >
      {/* Header bar with Cards vs Raw JSON toggle */}
      <div className="flex shrink-0 items-center justify-between border-b bg-muted/20 px-3 py-1.5">
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="gap-1 font-mono text-[10px]">
            <SparklesIcon className="size-3 text-emerald-500" />
            {kind === "groups"
              ? `${groups.length} groups (${total ?? 0} total hits)`
              : kind === "count"
                ? `Count: ${total ?? 0}`
                : total != null
                  ? `${total} result hits`
                  : "Live Qdrant Response"}
          </Badge>
        </div>

        <div className="flex items-center gap-1 rounded-lg border bg-muted/40 p-0.5">
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
      <div className="min-h-0 flex-1 overflow-auto p-3">
        {viewMode === "json" || kind === "none" ? (
          <JsonViewer value={responseStr} className="h-full" />
        ) : kind === "count" ? (
          <Card className="border-emerald-500/30 bg-card/60">
            <CardContent className="flex items-center justify-between p-5">
              <span className="font-mono text-xs text-muted-foreground">
                Matching points
              </span>
              <span className="font-mono text-3xl font-semibold text-emerald-500 tabular-nums">
                {total ?? 0}
              </span>
            </CardContent>
          </Card>
        ) : kind === "groups" && groups.length === 0 ? (
          <Card className="border-border/60 bg-card/60">
            <CardContent className="p-5 text-center font-mono text-xs text-muted-foreground">
              No result groups
            </CardContent>
          </Card>
        ) : kind === "groups" ? (
          /* Render Grouped Query Result Buckets */
          <div className="flex flex-col gap-4">
            {groups.map((group, gIdx) => (
              <Card
                key={`group-${gIdx}-${group.groupKey}`}
                className="overflow-hidden border-emerald-500/30 bg-card/60"
              >
                <CardHeader className="flex flex-row items-center justify-between border-b bg-muted/30 px-3 py-2.5">
                  <CardTitle className="flex items-center gap-2 font-mono text-xs font-semibold">
                    <LayersIcon className="size-3.5 text-emerald-500" />
                    <span>
                      Group:{" "}
                      <span className="font-bold text-emerald-400">
                        {group.groupKey}
                      </span>
                    </span>
                  </CardTitle>
                  <Badge variant="secondary" className="font-mono text-[10px]">
                    {group.hits.length} hit{group.hits.length > 1 ? "s" : ""}
                  </Badge>
                </CardHeader>
                <CardContent className="flex flex-col gap-2.5 p-3">
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
        ) : hits.length === 0 ? (
          <Card className="border-border/60 bg-card/60">
            <CardContent className="p-5 text-center font-mono text-xs text-muted-foreground">
              No matching points
            </CardContent>
          </Card>
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
      const lat =
        typeof obj.lat === "number" ? obj.lat.toFixed(4) : String(obj.lat)
      const lon =
        typeof obj.lon === "number" ? obj.lon.toFixed(4) : String(obj.lon)
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

  const scorePct =
    hit.score != null ? Math.min(Math.max(hit.score, 0), 1) * 100 : null
  const copyId = `hit-${hit.id}-${idx}`

  return (
    <Card
      size="sm"
      className="overflow-hidden border-border/60 transition-colors hover:border-primary/40"
    >
      <CardContent className="flex flex-col gap-2 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2">
          <div className="flex items-center gap-2">
            <Badge
              variant="default"
              className="bg-primary/80 font-mono text-[10px]"
            >
              #{idx + 1}
            </Badge>
            <span className="font-mono text-xs text-muted-foreground">
              ID:{" "}
              <span className="font-semibold text-foreground">
                {String(hit.id)}
              </span>
            </span>
          </div>

          <div className="flex items-center gap-3">
            {hit.score != null && (
              <div className="flex items-center gap-2">
                <span className="font-mono text-xs font-semibold text-emerald-600 tabular-nums dark:text-emerald-400">
                  Score {hit.score.toFixed(4)}
                </span>
                {scorePct != null && (
                  <div className="h-1.5 w-16 overflow-hidden rounded-full bg-muted">
                    <div
                      className="h-full rounded-full bg-emerald-500"
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
              className="h-6 gap-1 px-1.5 font-mono text-[10px]"
            >
              {copiedKey === copyId ? (
                <CheckIcon className="size-3 text-emerald-500" />
              ) : (
                <CopyIcon className="size-3" />
              )}
              {copiedKey === copyId ? "Copied" : "Copy Hit"}
            </Button>
          </div>
        </div>

        {/* Text / Name Payload */}
        {textContent ? (
          <div className="flex items-start gap-2 rounded-md border bg-muted/20 p-2.5 font-mono text-xs leading-relaxed text-foreground/90">
            <FileTextIcon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            <span className="line-clamp-4">{textContent}</span>
          </div>
        ) : (
          <div className="text-xs text-muted-foreground italic">
            No text payload field previewable
          </div>
        )}

        {/* Metadata Payload Chips */}
        {payloadEntries.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 pt-1">
            <TagIcon className="size-3 text-muted-foreground" />
            {payloadEntries.slice(0, 12).map(([key, val]) => (
              <Badge
                key={key}
                variant="outline"
                className="gap-1 font-mono text-[10px]"
              >
                <span className="text-muted-foreground">{key}:</span>
                <span className="font-semibold text-foreground">
                  {formatPayloadVal(val)}
                </span>
              </Badge>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
