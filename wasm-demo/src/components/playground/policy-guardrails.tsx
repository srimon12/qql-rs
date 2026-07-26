import { useState } from "react"
import {
  CheckIcon,
  ChevronRightIcon,
  FingerprintIcon,
  Globe2Icon,
  LockKeyholeIcon,
  RouteIcon,
  ShieldCheckIcon,
  SlidersHorizontalIcon,
  SparklesIcon,
  TagsIcon,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { cn } from "@/lib/utils"
import type { PolicyConfig, PolicyValueType } from "@/lib/qql-types"

type PolicyControlProps = {
  config: PolicyConfig
  onUpdateConfig: (next: PolicyConfig) => void
}

type PolicyTemplate = {
  id: string
  label: string
  field: string
  value: string
  valueType: PolicyValueType
  shardKey?: string
  description: string
  icon: typeof ShieldCheckIcon
}

const POLICY_TEMPLATES: PolicyTemplate[] = [
  {
    id: "workspace",
    label: "Workspace boundary",
    field: "workspace_id",
    value: "ws_101",
    valueType: "string",
    shardKey: "ws_101",
    description: "Logical ACL plus optional physical shard routing.",
    icon: LockKeyholeIcon,
  },
  {
    id: "lifecycle",
    label: "Soft-delete safety",
    field: "deleted",
    value: "false",
    valueType: "boolean",
    description: "Hide tombstoned data from reads and mutations.",
    icon: ShieldCheckIcon,
  },
  {
    id: "residency",
    label: "Data residency",
    field: "region",
    value: "eu-west-1",
    valueType: "string",
    description: "Enforce regional governance before execution.",
    icon: Globe2Icon,
  },
  {
    id: "visibility",
    label: "Visibility ACL",
    field: "visibility",
    value: "public",
    valueType: "string",
    description: "Constrain untrusted and agent-authored queries.",
    icon: FingerprintIcon,
  },
  {
    id: "moderation",
    label: "Content safety",
    field: "moderation_status",
    value: "approved",
    valueType: "string",
    description: "Guarantee only approved corpus content is reachable.",
    icon: TagsIcon,
  },
  {
    id: "environment",
    label: "Environment scope",
    field: "env",
    value: "prod",
    valueType: "string",
    description: "Keep prod, staging, and previews isolated.",
    icon: RouteIcon,
  },
]

const OPERATORS = ["=", ">", ">=", "<", "<="]

export function PolicyControl({ config, onUpdateConfig }: PolicyControlProps) {
  const [open, setOpen] = useState(false)

  return (
    <>
      <div className="flex items-center gap-1">
        <Button
          variant={config.enabled ? "default" : "outline"}
          size="sm"
          onClick={() => onUpdateConfig({ ...config, enabled: !config.enabled })}
          aria-pressed={config.enabled}
          className={cn(
            "gap-1.5 rounded-lg font-mono text-[11px]",
            config.enabled &&
              "bg-emerald-600 text-white shadow-sm shadow-emerald-500/20 hover:bg-emerald-500"
          )}
        >
          <ShieldCheckIcon className="size-3.5" />
          <span className="hidden sm:inline">Policy guardrail</span>
          <span className="sm:hidden">Policy</span>
          {config.enabled && <span className="size-1.5 rounded-full bg-emerald-200" />}
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => setOpen(true)}
          className="rounded-lg text-muted-foreground"
          aria-label="Configure runtime policy guardrail"
        >
          <SlidersHorizontalIcon className="size-3.5" />
        </Button>
      </div>

      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent
          side="right"
          className="w-full overflow-y-auto border-l bg-background sm:max-w-[520px]"
        >
          <SheetHeader className="border-b px-5 py-5 pr-14">
            <div className="mb-2 flex size-9 items-center justify-center rounded-xl border border-emerald-500/30 bg-emerald-500/10 text-emerald-500">
              <ShieldCheckIcon className="size-4" />
            </div>
            <SheetTitle className="text-lg font-semibold">Runtime policy engine</SheetTitle>
            <SheetDescription className="max-w-md leading-relaxed">
              Inject a mandatory predicate into the parsed AST before planning. QQL recursively
              carries it through CTEs, prefetch trees, reads, and scoped mutations—without changing
              the query in the editor.
            </SheetDescription>
          </SheetHeader>

          <div className="space-y-6 px-5 py-5">
            <section className="space-y-3" aria-labelledby="policy-recipes">
              <div className="flex items-end justify-between gap-3">
                <div>
                  <h3 id="policy-recipes" className="text-sm font-semibold">
                    Start with a policy recipe
                  </h3>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Tenant isolation is one use case—not the whole capability.
                  </p>
                </div>
                <Badge variant="outline" className="rounded-md font-mono text-[9px]">
                  inject_filter()
                </Badge>
              </div>

              <div className="grid gap-2 sm:grid-cols-2">
                {POLICY_TEMPLATES.map((template) => {
                  const Icon = template.icon
                  const active =
                    config.field === template.field &&
                    config.value === template.value &&
                    config.valueType === template.valueType
                  return (
                    <button
                      key={template.id}
                      type="button"
                      onClick={() =>
                        onUpdateConfig({
                          ...config,
                          enabled: true,
                          field: template.field,
                          op: "=",
                          value: template.value,
                          valueType: template.valueType,
                          shardKey: template.shardKey ?? "",
                        })
                      }
                      className={cn(
                        "group min-h-24 rounded-xl border p-3 text-left transition-colors outline-none hover:border-foreground/20 hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring/50",
                        active && "border-emerald-500/40 bg-emerald-500/[0.07]"
                      )}
                    >
                      <div className="flex items-start gap-2.5">
                        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground group-hover:text-foreground">
                          <Icon className="size-4" />
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center justify-between gap-2 text-xs font-semibold">
                            {template.label}
                            {active ? (
                              <CheckIcon className="size-3.5 text-emerald-500" />
                            ) : (
                              <ChevronRightIcon className="size-3.5 text-muted-foreground/50" />
                            )}
                          </span>
                          <span className="mt-1 block text-[11px] leading-relaxed text-muted-foreground">
                            {template.description}
                          </span>
                        </span>
                      </div>
                    </button>
                  )
                })}
              </div>
            </section>

            <section className="space-y-3 border-t pt-5" aria-labelledby="custom-policy">
              <div>
                <h3 id="custom-policy" className="text-sm font-semibold">
                  Custom mandatory predicate
                </h3>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  Model the request context your gateway already knows.
                </p>
              </div>

              <div className="grid gap-3 sm:grid-cols-[1fr_auto]">
                <div className="space-y-1.5">
                  <Label htmlFor="policy-field" className="text-xs">
                    Payload field
                  </Label>
                  <Input
                    id="policy-field"
                    value={config.field}
                    onChange={(event) => onUpdateConfig({ ...config, field: event.target.value })}
                    placeholder="workspace_id"
                    className="rounded-lg bg-background font-mono text-xs"
                  />
                </div>
                <fieldset className="space-y-1.5">
                  <legend className="text-xs">Operator</legend>
                  <div className="flex overflow-hidden rounded-lg border bg-background">
                    {OPERATORS.map((operator) => (
                      <button
                        key={operator}
                        type="button"
                        onClick={() => onUpdateConfig({ ...config, op: operator })}
                        aria-pressed={config.op === operator}
                        className={cn(
                          "h-9 min-w-9 border-r px-2 font-mono text-xs last:border-r-0 hover:bg-muted focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                          config.op === operator && "bg-primary text-primary-foreground hover:bg-primary"
                        )}
                      >
                        {operator}
                      </button>
                    ))}
                  </div>
                </fieldset>
              </div>

              <div className="grid gap-3 sm:grid-cols-[1fr_9rem]">
                <div className="space-y-1.5">
                  <Label htmlFor="policy-value" className="text-xs">
                    Required value
                  </Label>
                  <Input
                    id="policy-value"
                    value={config.value}
                    onChange={(event) => onUpdateConfig({ ...config, value: event.target.value })}
                    placeholder="ws_101"
                    className="rounded-lg bg-background font-mono text-xs"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="policy-value-type" className="text-xs">
                    Value type
                  </Label>
                  <select
                    id="policy-value-type"
                    value={config.valueType}
                    onChange={(event) =>
                      onUpdateConfig({
                        ...config,
                        valueType: event.target.value as PolicyValueType,
                      })
                    }
                    className="h-9 w-full rounded-lg border bg-background px-3 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  >
                    <option value="string">String</option>
                    <option value="number">Number</option>
                    <option value="boolean">Boolean</option>
                  </select>
                </div>
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="policy-shard" className="text-xs">
                  Physical shard key <span className="font-normal text-muted-foreground">(optional)</span>
                </Label>
                <Input
                  id="policy-shard"
                  value={config.shardKey}
                  onChange={(event) => onUpdateConfig({ ...config, shardKey: event.target.value })}
                  placeholder="Use only when physical routing should mirror the policy"
                  className="rounded-lg bg-background font-mono text-xs"
                />
              </div>
            </section>

            <div className="rounded-xl border border-emerald-500/25 bg-emerald-500/[0.06] p-3">
              <div className="flex items-start gap-2.5">
                <SparklesIcon className="mt-0.5 size-4 shrink-0 text-emerald-500" />
                <div className="min-w-0">
                  <p className="text-xs font-semibold">Host-enforced, recursively propagated</p>
                  <code className="mt-1.5 block break-all font-mono text-[11px] text-emerald-700 dark:text-emerald-300">
                    WHERE {config.field || "field"} {config.op || "="}{" "}
                    {config.valueType === "string" ? `'${config.value || "value"}'` : config.value || "value"}
                  </code>
                </div>
              </div>
            </div>

            <Button
              onClick={() => onUpdateConfig({ ...config, enabled: !config.enabled })}
              variant={config.enabled ? "outline" : "default"}
              className="w-full rounded-lg"
            >
              <ShieldCheckIcon />
              {config.enabled ? "Disable runtime policy" : "Enable runtime policy"}
            </Button>
          </div>
        </SheetContent>
      </Sheet>
    </>
  )
}
