import { createTheme } from "@uiw/codemirror-themes"
import { tags as t } from "@lezer/highlight"

/** Editor theme that follows shadcn CSS variables (no custom index.css). */
export const playgroundLight = createTheme({
  theme: "light",
  settings: {
    background: "transparent",
    foreground: "var(--foreground)",
    caret: "var(--primary)",
    selection: "color-mix(in oklch, var(--primary) 18%, transparent)",
    selectionMatch: "color-mix(in oklch, var(--primary) 12%, transparent)",
    lineHighlight: "color-mix(in oklch, var(--muted) 80%, transparent)",
    gutterBackground: "transparent",
    gutterForeground: "var(--muted-foreground)",
    gutterBorder: "transparent",
    gutterActiveForeground: "var(--foreground)",
  },
  styles: [
    { tag: t.keyword, color: "var(--primary)", fontWeight: "600" },
    { tag: t.string, color: "oklch(0.55 0.15 145)" },
    { tag: t.number, color: "oklch(0.55 0.18 25)" },
    { tag: t.comment, color: "var(--muted-foreground)", fontStyle: "italic" },
    { tag: t.operator, color: "oklch(0.5 0.12 230)" },
    { tag: t.punctuation, color: "var(--muted-foreground)" },
    { tag: t.atom, color: "oklch(0.55 0.14 300)" },
    { tag: t.variableName, color: "var(--foreground)" },
  ],
})

export const playgroundDark = createTheme({
  theme: "dark",
  settings: {
    background: "transparent",
    foreground: "var(--foreground)",
    caret: "var(--primary)",
    selection: "color-mix(in oklch, var(--primary) 28%, transparent)",
    selectionMatch: "color-mix(in oklch, var(--primary) 18%, transparent)",
    lineHighlight: "color-mix(in oklch, var(--muted) 50%, transparent)",
    gutterBackground: "transparent",
    gutterForeground: "var(--muted-foreground)",
    gutterBorder: "transparent",
    gutterActiveForeground: "var(--foreground)",
  },
  styles: [
    { tag: t.keyword, color: "var(--primary)", fontWeight: "600" },
    { tag: t.string, color: "oklch(0.78 0.14 145)" },
    { tag: t.number, color: "oklch(0.78 0.12 25)" },
    { tag: t.comment, color: "var(--muted-foreground)", fontStyle: "italic" },
    { tag: t.operator, color: "oklch(0.78 0.1 230)" },
    { tag: t.punctuation, color: "var(--muted-foreground)" },
    { tag: t.atom, color: "oklch(0.8 0.12 300)" },
    { tag: t.variableName, color: "var(--foreground)" },
  ],
})
