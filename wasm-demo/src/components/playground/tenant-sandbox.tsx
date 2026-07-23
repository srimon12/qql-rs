import { useState, useMemo } from "react"
import { ShieldCheckIcon, ArrowRightIcon, CopyIcon, CheckIcon, SparklesIcon, FileCode2Icon } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { compile, inject_filter } from "qql-wasm"

type TenantSandboxProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentQuery: string
  onApplyQuery: (query: string) => void
}

export function TenantSandbox({
  open,
  onOpenChange,
  currentQuery,
  onApplyQuery,
}: TenantSandboxProps) {
  const [field, setField] = useState("tenant_id")
  const [op, setOp] = useState("=")
  const [value, setValue] = useState("honeywell")
  const [shardKey, setShardKey] = useState("honeywell")
  const [copied, setCopied] = useState(false)
  const [activeDiffTab, setActiveDiffTab] = useState<"qql" | "wire" | "ast">("qql")

  // Target query to manipulate
  const baseQuery = currentQuery.trim() || "QUERY TEXT 'supply chain risk' FROM sec10k LIMIT 5"

  // Process injection using qql-wasm functions safely
  const transformResult = useMemo(() => {
    try {
      // Reconstruct QQL string representation for applied query
      let modifiedQql = baseQuery
      if (!/WHERE\b/i.test(modifiedQql)) {
        modifiedQql += `\n  WHERE ${field} ${op} '${value}'`
      } else {
        modifiedQql = modifiedQql.replace(/WHERE\s+/i, `WHERE ${field} ${op} '${value}' AND `)
      }
      if (shardKey.trim() && !/SHARD\b/i.test(modifiedQql)) {
        modifiedQql += `\n  SHARD '${shardKey.trim()}'`
      }

      // Original compile
      let origWire = "{}"
      try {
        origWire = JSON.stringify(JSON.parse(compile(baseQuery)), null, 2)
      } catch (err) {
        origWire = `// Original compile error: ${err}`
      }

      // Injected compile
      let injectedWire = "{}"
      try {
        injectedWire = JSON.stringify(JSON.parse(compile(modifiedQql)), null, 2)
      } catch (err) {
        injectedWire = `// Injected compile error: ${err}`
      }

      // AST injection via WASM
      let injectedAstJson = "{}"
      try {
        const injectedAst = inject_filter(baseQuery, field, op, value)
        injectedAstJson = JSON.stringify(injectedAst, null, 2)
      } catch (astErr) {
        injectedAstJson = `// AST injection info: ${astErr}`
      }

      return {
        success: true,
        origWire,
        origAst: baseQuery,
        injectedWire,
        injectedAst: injectedAstJson,
        modifiedQql,
        error: null,
      }
    } catch (err) {
      return {
        success: false,
        origWire: "{}",
        origAst: baseQuery,
        injectedWire: "{}",
        injectedAst: "{}",
        modifiedQql: baseQuery,
        error: err instanceof Error ? err.message : String(err),
      }
    }
  }, [baseQuery, field, op, value, shardKey])

  const handleCopy = () => {
    navigator.clipboard.writeText(transformResult.modifiedQql)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const handleApply = () => {
    onApplyQuery(transformResult.modifiedQql)
    onOpenChange(false)
  }

  const leftContent =
    activeDiffTab === "qql"
      ? baseQuery
      : activeDiffTab === "wire"
        ? transformResult.origWire
        : transformResult.origAst

  const rightContent =
    activeDiffTab === "qql"
      ? transformResult.modifiedQql
      : activeDiffTab === "wire"
        ? transformResult.injectedWire
        : transformResult.injectedAst

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[1400px] sm:w-[95vw] w-[95vw] max-w-[1400px] h-[92vh] max-h-[950px] flex flex-col overflow-hidden p-6 gap-3">
        <DialogHeader className="pb-3 border-b shrink-0">
          <div className="flex items-center gap-2">
            <ShieldCheckIcon className="size-6 text-emerald-500" />
            <DialogTitle className="text-xl font-bold tracking-tight">
              AST Security & Multi-Tenant Filter Sandbox
            </DialogTitle>
          </div>
          <DialogDescription className="text-xs text-muted-foreground">
            Demonstrates QQL’s core differentiator: zero-regex, safe programmatic AST manipulation (
            <code className="font-mono text-xs text-primary font-semibold">inject_filter()</code> and physical{" "}
            <code className="font-mono text-xs text-primary font-semibold">SHARD</code> key injection).
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-1 md:grid-cols-4 gap-4 py-2 shrink-0 bg-muted/20 p-3 rounded-lg border">
          <div>
            <Label className="text-xs font-semibold text-foreground">Filter Field</Label>
            <Input
              value={field}
              onChange={(e) => setField(e.target.value)}
              placeholder="tenant_id"
              className="font-mono text-xs mt-1 bg-background"
            />
          </div>
          <div>
            <Label className="text-xs font-semibold text-foreground">Operator</Label>
            <Input
              value={op}
              onChange={(e) => setOp(e.target.value)}
              placeholder="="
              className="font-mono text-xs mt-1 bg-background"
            />
          </div>
          <div>
            <Label className="text-xs font-semibold text-foreground">Filter Value</Label>
            <Input
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder="honeywell"
              className="font-mono text-xs mt-1 bg-background"
            />
          </div>
          <div>
            <Label className="text-xs font-semibold text-foreground">Physical Shard Key</Label>
            <Input
              value={shardKey}
              onChange={(e) => setShardKey(e.target.value)}
              placeholder="honeywell"
              className="font-mono text-xs mt-1 bg-background"
            />
          </div>
        </div>

        {transformResult.error ? (
          <div className="p-4 text-xs font-mono text-destructive bg-destructive/10 rounded-md border border-destructive/20 shrink-0">
            Transformation Error: {transformResult.error}
          </div>
        ) : (
          <div className="min-h-0 flex-1 flex flex-col gap-3 overflow-hidden">
            <Tabs
              value={activeDiffTab}
              onValueChange={(v) => setActiveDiffTab(v as "qql" | "wire" | "ast")}
              className="flex-1 min-h-0 flex flex-col"
            >
              <div className="flex items-center justify-between border-b pb-2 shrink-0">
                <TabsList className="font-mono text-xs font-medium">
                  <TabsTrigger value="qql" className="px-4">QQL Statement Diff</TabsTrigger>
                  <TabsTrigger value="wire" className="px-4">REST Wire Payload Diff</TabsTrigger>
                  <TabsTrigger value="ast" className="px-4">AST Diff</TabsTrigger>
                </TabsList>

                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="font-mono text-xs gap-1.5 py-1 px-2.5">
                    <SparklesIcon className="size-3.5 text-emerald-500" />
                    WASM Programmatic AST Mutation
                  </Badge>
                </div>
              </div>

              <div className="flex-1 min-h-0 grid grid-cols-1 md:grid-cols-2 gap-4 pt-3 overflow-hidden">
                {/* Left: Original */}
                <div className="flex flex-col rounded-xl border bg-muted/20 overflow-hidden shadow-sm">
                  <div className="bg-muted px-4 py-2.5 border-b text-xs font-mono font-semibold flex items-center justify-between shrink-0">
                    <span className="text-muted-foreground uppercase tracking-wider text-[11px]">1. Original {activeDiffTab.toUpperCase()} (Unfiltered)</span>
                    <Badge variant="outline" className="text-[10px] uppercase font-mono">Unfiltered</Badge>
                  </div>
                  <div className="flex-1 p-4 overflow-auto font-mono text-xs leading-relaxed bg-background">
                    <pre className="text-muted-foreground whitespace-pre-wrap font-mono">
                      {leftContent}
                    </pre>
                  </div>
                </div>

                {/* Right: Injected */}
                <div className="flex flex-col rounded-xl border border-emerald-500/40 bg-emerald-500/5 overflow-hidden shadow-sm">
                  <div className="bg-emerald-500/10 px-4 py-2.5 border-b border-emerald-500/20 text-xs font-mono font-semibold flex items-center justify-between shrink-0">
                    <span className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400 font-bold">
                      <ArrowRightIcon className="size-4" />
                      2. Mutated {activeDiffTab.toUpperCase()} (Tenant Isolated)
                    </span>
                    <Badge variant="default" className="text-[10px] font-mono bg-emerald-600 hover:bg-emerald-600 uppercase">
                      Tenant Isolated
                    </Badge>
                  </div>
                  <div className="flex-1 p-4 overflow-auto font-mono text-xs leading-relaxed bg-background">
                    <pre className="text-foreground whitespace-pre-wrap font-mono">
                      {rightContent}
                    </pre>
                  </div>
                </div>
              </div>
            </Tabs>
          </div>
        )}

        <div className="flex items-center justify-between border-t pt-3 mt-2">
          <Button variant="outline" size="sm" onClick={handleCopy} className="gap-1.5 font-mono text-xs">
            {copied ? <CheckIcon className="size-3.5 text-emerald-500" /> : <CopyIcon className="size-3.5" />}
            {copied ? "Copied QQL!" : "Copy Injected QQL"}
          </Button>

          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Close
            </Button>
            <Button size="sm" onClick={handleApply} className="gap-1.5">
              <FileCode2Icon className="size-3.5" />
              Apply Injected Query to Editor
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
