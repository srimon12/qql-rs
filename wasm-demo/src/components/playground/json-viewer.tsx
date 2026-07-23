import CodeMirror from "@uiw/react-codemirror"
import { json } from "@codemirror/lang-json"
import { EditorView } from "@codemirror/view"
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
  const resolved =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme

  const text = value || placeholder || ""

  return (
    <div className={cn("h-full min-h-0 overflow-hidden", className)}>
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
