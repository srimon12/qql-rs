import { useState, useMemo, useCallback, useEffect, useRef } from "react"
import {
  SearchIcon,
  CheckIcon,
  XIcon,
  LayersIcon,
  DatabaseIcon,
  BookOpenIcon,
  SparklesIcon,
  SlidersHorizontalIcon,
  ArrowUpDownIcon,
} from "lucide-react"

import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { cn } from "@/lib/utils"
import {
  PRESETS,
  PRESET_CATEGORIES,
  searchPresets,
  getFeaturedPresets,
  type PresetId,
  type Preset,
  type PresetCategory,
  type PresetComplexity,
} from "@/lib/presets"

const COMPLEXITY_LABEL: Record<PresetComplexity, string> = {
  beginner: "Beginner",
  intermediate: "Intermediate",
  advanced: "Advanced",
}

const COMPLEXITY_ORDER: Record<PresetComplexity, number> = {
  beginner: 0,
  intermediate: 1,
  advanced: 2,
}

type PresetBrowserProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  activePresetId: PresetId
  onSelect: (id: PresetId) => void
}

export function PresetBrowser({ open, onOpenChange, activePresetId, onSelect }: PresetBrowserProps) {
  const [search, setSearch] = useState("")
  const [activeCategory, setActiveCategory] = useState<PresetCategory | null>(null)
  const [showFeaturedOnly, setShowFeaturedOnly] = useState(false)
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setSearch("")
      setActiveCategory(null)
      setShowFeaturedOnly(false)
      setTimeout(() => searchRef.current?.focus(), 100)
    }
  }, [open])

  const categoryCounts = useMemo(() => {
    const counts = new Map<PresetCategory, number>()
    const source = showFeaturedOnly ? getFeaturedPresets() : PRESETS
    for (const p of source) {
      counts.set(p.category, (counts.get(p.category) ?? 0) + 1)
    }
    return counts
  }, [showFeaturedOnly])

  const results = useMemo(() => {
    let list = search ? searchPresets(search) : showFeaturedOnly ? getFeaturedPresets() : [...PRESETS]
    if (activeCategory) {
      list = list.filter((p) => p.category === activeCategory)
    }
    list.sort((a, b) => COMPLEXITY_ORDER[a.complexity] - COMPLEXITY_ORDER[b.complexity])
    return list
  }, [search, activeCategory, showFeaturedOnly])

  const handleSelect = useCallback(
    (id: PresetId) => {
      onSelect(id)
      onOpenChange(false)
    },
    [onSelect, onOpenChange]
  )

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault()
        searchRef.current?.focus()
      }
    },
    []
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-[800px] sm:w-[90vw] lg:max-w-[960px]"
        showCloseButton={false}
      >
        <DialogHeader className="shrink-0 border-b px-5 py-4">
          <div className="flex items-center justify-between gap-4">
            <div>
              <DialogTitle className="font-heading text-base font-semibold">Browse Presets</DialogTitle>
              <DialogDescription className="mt-0.5 text-xs">
                Explore query patterns and load them into the editor
              </DialogDescription>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => onOpenChange(false)}
              className="shrink-0"
            >
              <XIcon />
              <span className="sr-only">Close</span>
            </Button>
          </div>

          <div className="relative mt-3">
            <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={searchRef}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Search presets by name, tag, or description..."
              className="h-9 rounded-xl pl-9 pr-20 text-sm"
            />
            <kbd className="absolute top-1/2 right-3 -translate-y-1/2 hidden items-center gap-0.5 rounded-md border bg-muted/50 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground sm:inline-flex">
              <span className="text-[9px]">&#8984;</span>K
            </kbd>
          </div>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-0 sm:flex-row">
          <aside className="flex shrink-0 flex-col gap-1 border-b px-4 py-3 sm:w-[200px] sm:border-b-0 sm:border-r sm:px-3 sm:py-4">
            <button
              onClick={() => {
                setActiveCategory(null)
                setShowFeaturedOnly(false)
              }}
              className={cn(
                "flex items-center gap-2 rounded-xl px-3 py-2 text-left text-xs font-medium transition-colors",
                activeCategory === null && !showFeaturedOnly
                  ? "bg-primary/10 text-primary"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground"
              )}
            >
              <LayersIcon className="size-3.5 shrink-0" />
              All Presets
              <span className="ml-auto text-[10px] tabular-nums text-muted-foreground/60">
                {categoryCounts.size}
              </span>
            </button>

            <button
              onClick={() => setShowFeaturedOnly((v) => !v)}
              className={cn(
                "flex items-center gap-2 rounded-xl px-3 py-2 text-left text-xs font-medium transition-colors",
                showFeaturedOnly
                  ? "bg-amber-500/10 text-amber-600 dark:text-amber-400"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground"
              )}
            >
              <SparklesIcon className="size-3.5 shrink-0" />
              Featured
              {showFeaturedOnly && <CheckIcon className="ml-auto size-3" />}
            </button>

            <div className="my-1 h-px bg-border/50" />

            <span className="px-3 text-[10px] font-medium tracking-wide text-muted-foreground/60 uppercase">
              Categories
            </span>

            <div className="flex flex-row gap-1 overflow-x-auto sm:flex-col">
              {PRESET_CATEGORIES.map((cat) => {
                const count = categoryCounts.get(cat.id) ?? 0
                if (count === 0) return null
                return (
                  <button
                    key={cat.id}
                    onClick={() => setActiveCategory(activeCategory === cat.id ? null : cat.id)}
                    className={cn(
                      "flex shrink-0 items-center gap-2 rounded-xl px-3 py-2 text-left text-xs font-medium transition-colors",
                      activeCategory === cat.id
                        ? "bg-primary/10 text-primary"
                        : "text-muted-foreground hover:bg-muted hover:text-foreground"
                    )}
                  >
                    <span className="truncate">{cat.label}</span>
                    <span className="ml-auto text-[10px] tabular-nums text-muted-foreground/60">{count}</span>
                  </button>
                )
              })}
            </div>
          </aside>

          <div className="flex min-h-0 flex-1 flex-col">
            {results.length === 0 ? (
              <div className="flex flex-1 items-center justify-center p-8">
                <div className="flex flex-col items-center gap-2 text-center">
                  <SearchIcon className="size-8 text-muted-foreground/30" />
                  <p className="text-sm font-medium text-muted-foreground">No presets found</p>
                  <p className="text-xs text-muted-foreground/60">
                    Try a different search term or clear filters
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-2"
                    onClick={() => {
                      setSearch("")
                      setActiveCategory(null)
                      setShowFeaturedOnly(false)
                    }}
                  >
                    Clear filters
                  </Button>
                </div>
              </div>
            ) : (
              <ScrollArea className="flex-1">
                <div className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-2 lg:grid-cols-2">
                  {results.map((preset) => (
                    <PresetCard
                      key={preset.id}
                      preset={preset}
                      isActive={preset.id === activePresetId}
                      onSelect={handleSelect}
                    />
                  ))}
                </div>
              </ScrollArea>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-between border-t px-4 py-2.5 text-[11px] text-muted-foreground">
          <span>{results.length} preset{results.length !== 1 ? "s" : ""}</span>
          <span>
            <kbd className="inline-flex items-center gap-0.5 rounded border bg-muted/50 px-1 py-0.5 font-mono text-[10px]">
              <span className="text-[9px]">&#8984;</span>K
            </kbd>{" "}
            search &middot; click to load
          </span>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function PresetCard({
  preset,
  isActive,
  onSelect,
}: {
  preset: Preset
  isActive: boolean
  onSelect: (id: PresetId) => void
}) {
  return (
    <button
      onClick={() => onSelect(preset.id)}
      className={cn(
        "flex w-full flex-col gap-2.5 rounded-xl border px-4 py-3.5 text-left transition-all outline-none",
        "hover:bg-accent/40 hover:border-foreground/20",
        "focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:border-ring",
        isActive
          ? "border-primary/50 bg-primary/[0.04] ring-1 ring-primary/30"
          : "border-border bg-card"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {preset.labelBadge && (
            <Badge
              variant="outline"
              className="shrink-0 border-primary/30 bg-primary/[0.06] px-1.5 py-0 font-mono text-[9px] font-semibold text-primary"
            >
              {preset.labelBadge}
            </Badge>
          )}
          <span className="truncate text-sm font-medium leading-tight">{preset.label}</span>
        </div>
        {isActive && (
          <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-primary text-[10px] text-primary-foreground">
            <CheckIcon className="size-3" />
          </span>
        )}
      </div>

      <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">{preset.description}</p>

      <div className="flex flex-wrap items-center gap-2">
        <Tag icon={<LayersIcon className="size-3" />} label={preset.category} />
        <Tag icon={<ArrowUpDownIcon className="size-3" />} label={COMPLEXITY_LABEL[preset.complexity]} />
        <Tag icon={<DatabaseIcon className="size-3" />} label={preset.dataset} />
        {preset.impact.reads && <Tag icon={<BookOpenIcon className="size-3" />} label="Read" />}
        {preset.impact.writes && <Tag icon={<SlidersHorizontalIcon className="size-3" />} label="Write" />}
        {preset.impact.schema && <Tag icon={<SlidersHorizontalIcon className="size-3" />} label="Schema" />}
      </div>

      {preset.tags.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {preset.tags.slice(0, 4).map((tag) => (
            <span
              key={tag}
              className="inline-flex h-5 items-center rounded-md bg-muted/60 px-1.5 text-[9px] font-medium text-muted-foreground/70"
            >
              {tag}
            </span>
          ))}
          {preset.tags.length > 4 && (
            <span className="inline-flex h-5 items-center px-1 text-[9px] text-muted-foreground/40">
              +{preset.tags.length - 4}
            </span>
          )}
        </div>
      )}
    </button>
  )
}

function Tag({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <span className="inline-flex items-center gap-1 rounded-md bg-muted/40 px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground/70">
      {icon}
      {label}
    </span>
  )
}
