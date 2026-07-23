import { useState } from "react"
import CodeMirror from "@uiw/react-codemirror"
import { json } from "@codemirror/lang-json"
import { EditorView } from "@codemirror/view"
import { CopyIcon, CheckIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useTheme } from "@/components/theme-provider"
import { playgroundDark, playgroundLight } from "@/lib/editor-theme"
import { cn } from "@/lib/utils"

type JsonViewerProps = {
  value: string
  className?: string
  placeholder?: string
}

export function JsonViewer({ value, className, placeholder }: JsonViewerProps) {
  const { theme } = useTheme()
  const [copied, setCopied] = useState(false)

  const resolved =
    theme === "system"
      ? typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme

  const text = value || placeholder || ""

  const handleCopy = () => {
    if (!text) return
    navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className={cn("relative h-full min-h-0 overflow-hidden group", className)}>
      {text && (
        <Button
          variant="secondary"
          size="xs"
          onClick={handleCopy}
          className="absolute top-2 right-4 z-10 font-mono text-[10px] gap-1 bg-background/80 backdrop-blur border shadow-sm opacity-80 group-hover:opacity-100 transition-opacity"
        >
          {copied ? <CheckIcon className="size-3 text-emerald-500" /> : <CopyIcon className="size-3" />}
          {copied ? "Copied" : "Copy JSON"}
        </Button>
      )}
      <CodeMirror
        value={text}
        height="100%"
        editable={false}
        basicSetup={{
          lineNumbers: true,
          foldGutter: true,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
        }}
        extensions={[json(), EditorView.lineWrapping]}
        theme={resolved === "dark" ? playgroundDark : playgroundLight}
        className="h-full text-sm [&_.cm-editor]:h-full [&_.cm-scroller]:font-mono"
      />
    </div>
  )
}
