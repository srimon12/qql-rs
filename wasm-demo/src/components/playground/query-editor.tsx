import { useMemo } from "react"
import CodeMirror from "@uiw/react-codemirror"
import { EditorView } from "@codemirror/view"
import { linter, type Diagnostic } from "@codemirror/lint"
import { keymap } from "@codemirror/view"
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands"
import { autocompletion } from "@codemirror/autocomplete"
import { useTheme } from "@/components/theme-provider"
import { qqlLanguage, qqlCompletions } from "@/lib/qql-language"
import { playgroundDark, playgroundLight } from "@/lib/editor-theme"
import type { AnalysisResult } from "@/lib/qql-types"
import { cn } from "@/lib/utils"

type QueryEditorProps = {
  value: string
  onChange: (value: string) => void
  analysis: AnalysisResult
  className?: string
  onExecute?: () => void
}

export function QueryEditor({
  value,
  onChange,
  analysis,
  className,
  onExecute,
}: QueryEditorProps) {
  const { theme } = useTheme()
  const resolved =
    theme === "system"
      ? typeof window !== "undefined" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme

  const extensions = useMemo(() => {
    const errorLinter = linter(() => {
      const diagnostics: Diagnostic[] = []
      const err = analysis.error
      if (!analysis.valid && err && err.start != null && err.end != null) {
        diagnostics.push({
          from: err.start,
          to: Math.max(err.end, err.start + 1),
          severity: "error",
          message: err.message
            ? `${err.code ?? "error"}: ${err.message}`
            : (err.code ?? "Parse error"),
        })
      }
      return diagnostics
    })

    return [
      qqlLanguage,
      autocompletion({ override: [qqlCompletions] }),
      history(),
      EditorView.lineWrapping,
      errorLinter,
      keymap.of([
        {
          key: "Mod-Enter",
          run: () => {
            onExecute?.()
            return true
          },
        },
        {
          key: "Ctrl-Enter",
          run: () => {
            onExecute?.()
            return true
          },
        },
        {
          key: "Cmd-Enter",
          run: () => {
            onExecute?.()
            return true
          },
        },
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      EditorView.theme({
        "&": { height: "100%", fontSize: "13.5px" },
        ".cm-scroller": {
          fontFamily:
            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
          lineHeight: "1.6",
        },
        ".cm-content": { padding: "12px 0" },
        ".cm-gutters": { border: "none" },
      }),
    ]
  }, [analysis, onExecute])

  return (
    <div className={cn("h-full min-h-0 overflow-hidden", className)}>
      <CodeMirror
        value={value}
        height="100%"
        theme={resolved === "dark" ? playgroundDark : playgroundLight}
        extensions={extensions}
        basicSetup={{
          lineNumbers: true,
          foldGutter: true,
          highlightActiveLine: true,
          highlightActiveLineGutter: true,
          bracketMatching: true,
          closeBrackets: true,
          autocompletion: true,
          searchKeymap: true,
        }}
        onChange={onChange}
        className="h-full [&_.cm-editor]:h-full [&_.cm-editor]:outline-none"
      />
    </div>
  )
}
