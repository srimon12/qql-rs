import { useState, useRef, useEffect } from "react"
import { ShieldCheckIcon, SparklesIcon, SlidersHorizontalIcon, XIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import type { TenantConfig } from "@/lib/qql-types"

type TenantControlProps = {
  tenantConfig: TenantConfig
  onUpdateConfig: (next: TenantConfig) => void
}

export function TenantControl({
  tenantConfig,
  onUpdateConfig,
}: TenantControlProps) {
  const [open, setOpen] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)
  const { field, op, value, shardKey, enabled } = tenantConfig

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    if (open) {
      document.addEventListener("mousedown", handleClickOutside)
    }
    return () => document.removeEventListener("mousedown", handleClickOutside)
  }, [open])

  return (
    <div ref={containerRef} className="relative flex items-center gap-1.5">
      {/* Quick Toggle Button */}
      <Button
        variant={enabled ? "default" : "outline"}
        size="sm"
        onClick={() => onUpdateConfig({ ...tenantConfig, enabled: !enabled })}
        className={`font-mono text-xs gap-1.5 transition-all ${
          enabled
            ? "bg-emerald-600 hover:bg-emerald-500 text-white shadow-sm shadow-emerald-500/20"
            : "text-emerald-600 dark:text-emerald-400 bg-emerald-500/5 hover:bg-emerald-500/10 border-emerald-500/30"
        }`}
      >
        <ShieldCheckIcon className="size-3.5" />
        {enabled ? "Tenant Isolation (Active)" : "Tenant Isolation"}
      </Button>

      {/* Settings Popover Button */}
      <Button
        variant="outline"
        size="icon-sm"
        onClick={() => setOpen(!open)}
        className={`h-8 w-8 transition-colors ${
          open ? "bg-accent text-accent-foreground border-accent" : "text-muted-foreground hover:text-foreground"
        }`}
        title="Tenant Isolation Settings"
      >
        <SlidersHorizontalIcon className="size-3.5" />
      </Button>

      {/* Settings Floating Card */}
      {open && (
        <div className="absolute right-0 top-10 z-50 w-80 rounded-lg border bg-popover p-4 shadow-xl text-popover-foreground space-y-3 font-mono text-xs animate-in fade-in-50 zoom-in-95">
          <div className="flex items-center justify-between border-b pb-2">
            <div className="flex items-center gap-1.5">
              <ShieldCheckIcon className="size-4 text-emerald-500" />
              <span className="font-bold text-foreground">Tenant AST Isolation</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Badge
                variant="outline"
                className={`text-[10px] ${
                  enabled
                    ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/40"
                    : "text-muted-foreground"
                }`}
              >
                {enabled ? "Active" : "Disabled"}
              </Badge>
              <button
                onClick={() => setOpen(false)}
                className="text-muted-foreground hover:text-foreground p-0.5 rounded"
              >
                <XIcon className="size-3.5" />
              </button>
            </div>
          </div>

          <div className="space-y-2.5 pt-1">
            <div className="space-y-1">
              <Label className="text-[11px] text-muted-foreground">Payload Filter Field</Label>
              <Input
                value={field}
                onChange={(e) => onUpdateConfig({ ...tenantConfig, field: e.target.value })}
                placeholder="tenant_id"
                className="h-8 font-mono text-xs bg-background"
                autoFocus
              />
            </div>

            <div className="space-y-1">
              <Label className="text-[11px] text-muted-foreground">Operator & Filter Value</Label>
              <div className="flex items-center gap-1.5">
                <Input
                  value={op}
                  onChange={(e) => onUpdateConfig({ ...tenantConfig, op: e.target.value })}
                  placeholder="="
                  className="h-8 w-12 font-mono text-xs text-center shrink-0 bg-background"
                />
                <Input
                  value={value}
                  onChange={(e) => onUpdateConfig({ ...tenantConfig, value: e.target.value })}
                  placeholder="honeywell"
                  className="h-8 font-mono text-xs flex-1 bg-background"
                />
              </div>
            </div>

            <div className="space-y-1">
              <Label className="text-[11px] text-muted-foreground">Physical Shard Routing Key</Label>
              <Input
                value={shardKey}
                onChange={(e) => onUpdateConfig({ ...tenantConfig, shardKey: e.target.value })}
                placeholder="honeywell"
                className="h-8 font-mono text-xs bg-background"
              />
            </div>
          </div>

          <div className="pt-2 border-t flex items-center justify-between text-[10px] text-muted-foreground">
            <span className="flex items-center gap-1">
              <SparklesIcon className="size-3 text-emerald-500" />
              AST injected on execute
            </span>
            <Button
              variant="ghost"
              size="xs"
              onClick={() => onUpdateConfig({ ...tenantConfig, enabled: !enabled })}
              className="h-6 px-2 text-[10px] text-emerald-500 font-semibold hover:bg-emerald-500/10"
            >
              {enabled ? "Turn Off" : "Turn On"}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
