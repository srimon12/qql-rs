import { useEffect, useState } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type { EmbedProvider, PlaygroundSettings } from "@/lib/qql-types"
import {
  BROWSER_EMBED_DIM,
  BROWSER_EMBED_MODEL,
} from "@/lib/browser-embedder"

type SettingsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  settings: PlaygroundSettings
  onSave: (settings: PlaygroundSettings) => void | Promise<void>
  saving?: boolean
}

export function SettingsDialog({
  open,
  onOpenChange,
  settings,
  onSave,
  saving,
}: SettingsDialogProps) {
  const [draft, setDraft] = useState(settings)

  useEffect(() => {
    if (open) setDraft(settings)
  }, [open, settings])

  const set = <K extends keyof PlaygroundSettings>(
    key: K,
    value: PlaygroundSettings[K]
  ) => setDraft((d) => ({ ...d, [key]: value }))

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" showCloseButton>
        <DialogHeader>
          <DialogTitle>Connection & embeddings</DialogTitle>
          <DialogDescription>
            Offline-first: all-MiniLM-L6-v2 runs in the browser (same 384-d
            family as SEC 10-K). Point Qdrant at your local cluster for{" "}
            <code className="font-mono text-xs">sec10k</code>.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-5">
          <section className="grid gap-3">
            <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Qdrant
            </h3>
            <div className="grid gap-3">
              <div className="grid gap-1.5">
                <Label htmlFor="qdrant-url">REST URL</Label>
                <Input
                  id="qdrant-url"
                  value={draft.qdrantUrl}
                  onChange={(e) => set("qdrantUrl", e.target.value)}
                  placeholder="http://localhost:6333"
                  className="font-mono text-xs"
                />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="qdrant-key">API key (optional)</Label>
                <Input
                  id="qdrant-key"
                  type="password"
                  value={draft.qdrantKey}
                  onChange={(e) => set("qdrantKey", e.target.value)}
                  placeholder="Optional"
                  className="font-mono text-xs"
                />
              </div>
            </div>
          </section>

          <section className="grid gap-3">
            <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Embedder
            </h3>
            <div className="grid gap-1.5">
              <Label>Provider</Label>
              <Select
                value={draft.embedProvider}
                onValueChange={(v) => {
                  const p = v as EmbedProvider
                  set("embedProvider", p)
                  if (p === "browser") {
                    set("embedDim", BROWSER_EMBED_DIM)
                  }
                }}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="browser">
                    In-browser MiniLM (offline-first)
                  </SelectItem>
                  <SelectItem value="http">
                    External HTTP (Ollama / OpenAI / LM Studio)
                  </SelectItem>
                  <SelectItem value="none">None (raw vectors only)</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {draft.embedProvider === "browser" && (
              <div className="rounded-xl border bg-muted/40 p-3 text-xs leading-relaxed text-muted-foreground">
                <p>
                  Model:{" "}
                  <code className="font-mono text-foreground">
                    {BROWSER_EMBED_MODEL}
                  </code>
                </p>
                <p className="mt-1">
                  Output:{" "}
                  <code className="font-mono text-foreground">
                    {BROWSER_EMBED_DIM}-d
                  </code>{" "}
                  mean-pooled, L2-normalized — matches{" "}
                  <code className="font-mono">sec10k</code> dense vectors.
                  First load downloads weights into the browser cache
                  (WebGPU when available, else WASM).
                </p>
              </div>
            )}

            {draft.embedProvider === "http" && (
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="grid gap-1.5 sm:col-span-2">
                  <Label htmlFor="embed-url">Endpoint URL</Label>
                  <Input
                    id="embed-url"
                    value={draft.embedUrl}
                    onChange={(e) => set("embedUrl", e.target.value)}
                    className="font-mono text-xs"
                  />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="embed-model">Model</Label>
                  <Input
                    id="embed-model"
                    value={draft.embedModel}
                    onChange={(e) => set("embedModel", e.target.value)}
                    className="font-mono text-xs"
                  />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="embed-dim">Dimension</Label>
                  <Input
                    id="embed-dim"
                    type="number"
                    value={draft.embedDim}
                    onChange={(e) =>
                      set("embedDim", Number(e.target.value) || 384)
                    }
                    className="font-mono text-xs"
                  />
                </div>
                <div className="grid gap-1.5 sm:col-span-2">
                  <Label htmlFor="embed-key">API key (optional)</Label>
                  <Input
                    id="embed-key"
                    type="password"
                    value={draft.embedKey}
                    onChange={(e) => set("embedKey", e.target.value)}
                    placeholder="Optional"
                    className="font-mono text-xs"
                  />
                </div>
              </div>
            )}
          </section>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={saving}
            onClick={async () => {
              await onSave(draft)
              onOpenChange(false)
            }}
          >
            {saving ? "Applying…" : "Save & apply"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
