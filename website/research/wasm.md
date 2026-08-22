QQL WASM Demo — UI / Architecture Audit Report
Date: 2026-07-31  
Source repo: qql-wasm-demo (standalone SPA)  
Target integration: qql-rs/website (Astro 7 + Starlight docs)  
Audit scope: Full read-only; zero modifications.
1. Framework, Build & Package Versions
Layer	Technology	Version
Build	Vite	^8 (Rolldown-based)
Framework	React	^19.2.6
Language	TypeScript	~6 (TypeScript 6)
CSS	Tailwind CSS	^4 (no PostCSS config; @tailwindcss/vite plugin)
UI primitives	shadcn/ui (Base UI style base-maia)	shadcn ^4.14.0
Base UI (Radix replacement)	@base-ui/react	^1.6.0
Animation	tw-animate-css	^1.4.0
Editor	CodeMirror 6 via @uiw/react-codemirror	^4.25.11
WASM engine	qql-wasm (npm)	^0.2.1
Browser embeddings	@huggingface/transformers	^4.2.0
Icons	lucide-react	^1.25.0
Resizable panels	react-resizable-panels	^4.12.2
Toasts	sonner	^2.0.7
Fonts	@fontsource-variable/inter, @fontsource/instrument-serif, @fontsource/jetbrains-mono	^5.x
Linting	ESLint ^10 + typescript-eslint + react-hooks + react-refresh	—
Formatter	Prettier ^3.8.3 + prettier-plugin-tailwindcss	—
Deploy	Cloudflare Pages via Wrangler	wrangler.toml included
CI/CD	GitHub Actions (.github/workflows/ci.yml, deploy.yml)	—
Key compat issue: The demo uses TypeScript ~6 and Vite ^8; the target Astro site (qql-rs/website) uses TypeScript ^5.9.0 and Astro ^7.0.6 (itself on Vite 6.x). Direct import of demo components will require careful tsconfig alignment or a separate build step.
2. Source Tree Map (significant files only)
qql-wasm-demo/
├── index.html                          # SPA entry, SEO meta, theme FOUC guard
├── package.json                        # Dependencies & scripts
├── vite.config.ts                      # Aliases, WASM deps exclude, COOP/COEP, worker ES
├── tsconfig.json / tsconfig.app.json   # Strict TS → es2023 target, bundler resolution
├── components.json                     # shadcn config (style: base-maia, baseColor: mist)
├── pnpm-workspace.yaml                 # allowBuilds for esbuild/onnx (blocked)
├── wrangler.toml                       # CF Pages config
├── AGENT.md                            # Internal contributor guide (245 lines)
├── README.md                           # User-facing docs (220 lines)
├── scripts/
│   └── validate-presets.mjs            # CI gate: analyze() every preset with qql-wasm
├── public/
│   ├── _headers                        # COOP/COEP, cache headers for CF
│   ├── favicon.ico, og-image.png, robots.txt, sitemap.xml, vite.svg
└── src/
    ├── main.tsx                        # React root → <ThemeProvider><App /></ThemeProvider>
    ├── App.tsx                         # 913-line monolith: shell, layout, state, all logic
    ├── index.css                       # Tailwind 4 + shadcn/tailwind + brand tokens (175 lines)
    ├── hooks/
    │   └── use-qql.ts                  # Core hook: WASM init, analyze(), execute(), settings
    ├── lib/
    │   ├── qql-types.ts                # All TypeScript types & settings persistence
    │   ├── qql-language.ts             # CodeMirror StreamLanguage for QQL (264 lines)
    │   ├── editor-theme.ts             # CM light/dark themes from CSS variables
    │   ├── browser-embedder.ts         # Lazy Transformers.js MiniLM pipeline (249 lines)
    │   ├── presets.ts                  # Preset catalog helpers
    │   ├── site.ts                     # Site config, nav URLs, SEO helpers
    │   └── utils.ts                    # cn() helper (clsx + tailwind-merge)
    ├── data/
    │   └── presets.json                # 557-line preset catalog (queries + metadata)
    ├── components/
    │   ├── theme-provider.tsx          # Inline theme provider (231 lines, no next-themes)
    │   ├── ui/                         # 18 shadcn Base UI primitives (3,000+ lines total)
    │   │   ├── button.tsx, badge.tsx, card.tsx, dialog.tsx, sheet.tsx
    │   │   ├── tabs.tsx, select.tsx, tooltip.tsx, input.tsx, label.tsx
    │   │   ├── separator.tsx, resizable.tsx, scroll-area.tsx, table.tsx
    │   │   ├── alert.tsx, textarea.tsx, dropdown-menu.tsx, sonner.tsx
    │   └── playground/                # 12 feature components (2,600+ lines)
    │       ├── query-editor.tsx        # CodeMirror QQL editor wrapper
    │       ├── inspector.tsx           # 7-tab inspector dispatcher
    │       ├── plan-view.tsx           # REST route + explain card
    │       ├── metrics-view.tsx        # Embedder status + timing grid
    │       ├── json-viewer.tsx         # Read-only CM JSON viewer w/ copy
    │       ├── results-cards.tsx       # Search hit/group/count renderer (482 lines)
    │       ├── tokens-table.tsx        # Lexer token table
    │       ├── preset-browser.tsx      # Full preset dialog (344 lines)
    │       ├── settings-dialog.tsx     # Connection & embedder settings
    │       ├── code-exporter.tsx       # SDK code generation dialog
    │       ├── audit-bar.tsx           # Compiler audit security strip
    │       └── policy-guardrails.tsx   # Policy injection sheet (341 lines)
    └── dist/                          # Build output (34 asset files)
3. WASM Initialization & Data Flow
Init Sequence (use-qql.ts)
App mount
  → useEffect in useQql()
  → await init() from "qql-wasm"          # Loads qql_wasm_bg.wasm
  → loadSettings() from localStorage      # Restore saved PlaygroundSettings
  → configureClient(cfg)                  # new Client(url, key)
  →   if embedProvider === "browser":
        client.setEmbedder(browserEmbedderFn)   # Closure over lazy Transformers.js
  →   if embedProvider === "http":
        client.setHttpEmbedder(url, model, dim, key)
  → setReady(true)
Query Flow
User types in CodeMirror
  → debouncedAnalyze (80ms debounce in useDebouncedCallback)
  → runAnalysis(source, policyConfig) in useQql
  →   if policy enabled: Stmt(source) → injectFilter() → compileRoute()
  →   else: analyze(source) from qql-wasm
  → setAnalysis(result), setParseMs(elapsed)
  → CodeMirror linter reads analysis.error.start/.end for underlines

User clicks Execute / Ctrl+Enter
  → onExecute()
  → execute(source, policyConfig) in useQql
  →   configureClient(cfg) if embedder changed
  →   if policy: Stmt(source) → injectFilter() → client.executeStmt(stmt)
  →   else: client.execute(source)
  →   probe embed timings via probeRef
  → setResponse(report), setMetrics(execMetrics)
Key Architectural Decisions
- No Web Worker for WASM — qql-wasm initializes and runs on the main thread. The WASM binary is loaded as a Vite asset (assetsInclude: ["**/*.wasm"]). COOP/COEP headers (credentialless + same-origin) are required for SharedArrayBuffer support (WASM threading).
- @huggingface/transformers is dynamically imported inside ensureBrowserEmbedder() — not in the main chunk. Vite excludes it with optimizeDeps.exclude: ["qql-wasm", "@huggingface/transformers"].
- Client retirement — old Client instances are queued in retiredClientsRef and freed with .free() once no active executions remain (reference counting via activeExecutionsRef).
- analyze() is synchronous (WASM function), quick enough for 80ms debounce.
- execute() is async (calls client.execute() which goes over the network + WASM).
Critical APIs (qql-wasm v0.2.1)
API	Signature	Cost
init()	() => Promise<void>	One-time
analyze(qql)	(source: string) => AnalysisResult	Sync, cheap
new Client(url, apiKey)	(string, string|null)	Constructor
client.setEmbedder(fn)	((texts: string[]) => number[][]) => void	Sync
client.setHttpEmbedder(url, model, dim, key)	(string, string, number, string|null) => void	Sync
client.execute(qql)	(string) => Promise<unknown>	Async, network
client.executeStmt(stmt)	(Stmt) => Promise<unknown>	Async, network
new Stmt(source)	(string) => Stmt	Sync
stmt.injectFilter(field, op, value)	(string, string, string|number|boolean) => void	Sync
stmt.free()	() => void	Memory cleanup
4. Playground Functionality Audit
4.1 Editor (query-editor.tsx)
- CodeMirror 6 via @uiw/react-codemirror, height 100%, line wrapping.
- QQL StreamLanguage (qql-language.ts): 142 keywords from qql-core, full comment/string/number/punctuation/operator tokenization.
- Autocomplete: qqlCompletions() returns full snippets (HYBRID, TEXT, UPSERT, etc.) plus keyword completions.
- Keybinding: Mod-Enter/Ctrl-Enter/Cmd-Enter mapped to onExecute.
- Live lint: linter() reads analysis.error.start/end from last analyze() → error underlines with message.
- Editor theme: playgroundLight/playgroundDark dynamically created from @uiw/codemirror-themes, using CSS variable references (var(--foreground), var(--primary)) for theme-following colors.
- State: uncontrolled via value + onChange pattern.
4.2 Result Status (in-header badge)
- Status badge in toolbar shows: Loading WASM | Empty | Valid | N statements | error code.
- Color-coded: green check for valid, red alert for error, outline for empty.
- Live parse-time display: parse X.X ms plus optional · exec X ms after execution.
4.3 Error Handling
- Inline linter errors via CM lint gutter underlines at exact source spans.
- Bottom error strip in EditorPanel when !analysis.valid && error.message — red border, error code + message.
- Execute errors caught in execute() try/catch → setResponse({ error: message }) → displayed in Response tab.
- Init errors render a full-screen destructive Alert.
- Policy injection errors return POLICY_MULTI_STATEMENT or POLICY_INJECTION codes.
4.4 Loading States
- WASM init: EditorPanel shows centered spinner "Loading qql-wasm" until ready === true.
- Browser embedder: Badge shows MiniLM X% during download; progress bar in Metrics tab; status text updates.
- Execute: Button shows Loader2Icon spinner and is disabled while executing === true.
- Preset validation: CI-only (scripts/validate-presets.mjs).
4.5 Inspector Tabs (inspector.tsx)
Tab	Content	Source
Plan	REST method + path badge, multi-statement selector, explanation	plan-view.tsx
Metrics	Embedder status, timing grid (parse, embed, network, total)	metrics-view.tsx
Wire JSON	Qdrant REST request body (current statement)	json-viewer.tsx
AST	Parsed statement tree (current statement)	json-viewer.tsx
Tokens	Lexer table (kind, literal, pos, end, len)	tokens-table.tsx
Explain	Human-readable plan text	<pre> directly
Response	Live Qdrant response (cards or raw JSON)	results-cards.tsx
4.6 Presets (preset-browser.tsx)
- Large dialog with search, category sidebar, featured toggle.
- 557-line presets.json catalog with 10 categories, ~40+ presets.
- searchPresets() filters by label, description, teaching, tags.
- getFeaturedPresets() for the sparkle filter.
- PresetCard shows badge, label, description, metadata tags.
- Validate on select via handleOpenChange → reset state + focus search.
- Keyboard: ⌘/Ctrl+K opens (global listener in App), ⌘/Ctrl+K focuses search inside.
- Data validated at CI time via scripts/validate-presets.mjs (imports qql-wasm, calls analyze() on every query).
4.7 Settings (settings-dialog.tsx)
- Qdrant URL + API key fields.
- Embedder selector: browser (default) / HTTP / none.
- Browser mode shows model info card (model name, dim).
- HTTP mode shows URL, model, dimension, API key fields.
- Save button calls updateSettings() → saveSettings() (localStorage) + configureClient() (rebinds WASM client).
- Legacy provider migration (openai/remote → http).
- Settings key: qql-playground-settings-v2.
4.8 URL / Share / State Handling
- Hash-based sharing: window.location.hash = #q=<encoded_query>.
- getInitialQuery() reads #q=... on load, falls back to default preset.
- Share button copies https://.../playground/#q=<encoded_query> to clipboard.
- No deep-link for other state (tab, settings, policy) — only the query string.
- Settings persist separately in localStorage (qql-playground-settings-v2).
4.9 Policy Guardrails (policy-guardrails.tsx)
- Right-side Sheet with 6 templates + custom configuration.
- Templates: workspace boundary, soft-delete safety, data residency, visibility ACL, content safety, environment scope.
- Custom: field name, operator (=, >, >=, <, <=), value, value type (string/number/boolean), optional shard key.
- Live badge in toolbar: "Policy guardrail" with green indicator when active.
- Status strip below toolbar when active: "Host policy enforced" with WHERE clause.
- Enforcement: runAnalysis() calls stmt.injectFilter() if policy enabled. Multi-statement scripts are rejected with POLICY_MULTI_STATEMENT error.
4.10 SDK Code Exporter (code-exporter.tsx)
- Dialog with 4 tabs: Python, Node.js, Rust, cURL.
- Generates SDK snippets from the current query + analysis.
- cURL uses compiled path + method + payload from the current statement route.
- Copy button with confirmation state.
4.11 Audit Bar (audit-bar.tsx)
- Thin strip below main workspace: shows AST Valid/Syntax Error badge, Filter Present/Unfiltered, Physical Shard/All Shards, statement count.
- Security warnings for unfiltered queries.
4.12 Result Cards (results-cards.tsx)
- Parses Qdrant response into SearchHit[], GroupedResult[], or count.
- Toggle between card view and raw JSON.
- Hit cards show: rank number, ID, score with progress bar, text content preview, metadata chips.
- Group view shows group key header + per-group hit cards.
- Count view shows plain number.
5. Component Hierarchy
<ThemeProvider defaultTheme="dark">
  <TooltipProvider>
    <App>                                    [913 lines in App.tsx]
      ├── <header>                           Site chrome (brand logo, nav, badges)
      │     ├── WASM badge, Embed badge
      │     ├── Status badge, Parse latency
      │     ├── Share, Settings, Theme toggle, Docs, GitHub
      ├── <div role="toolbar">               Scenario bar
      │     ├── "Explore capabilities" button → PresetBrowser (Dialog)
      │     ├── Active preset label + badge + metadata
      │     └── PolicyControl → PolicyGuardrails (Sheet)
      ├── <div>                              Policy enforcement strip (conditional)
      ├── <main>                             Workspace
      │     ├── ResizablePanelGroup (desktop) or stack (mobile)
      │     │   ├── EditorPanel
      │     │   │   ├── QueryEditor (CodeMirror)
      │     │   │   └── Error strip (conditional)
      │     │   └── Inspector
      │     │       ├── Tabs (Plan, Metrics, Wire, AST, Tokens, Explain, Response)
      │     │       ├── PlanView, MetricsView, JsonViewer, TokensTable
      │     │       └── ResultCards
      ├── AuditBar
      ├── <footer>                            Status bar
      ├── PresetBrowser (Dialog)
      ├── SettingsDialog (Dialog)
      └── CodeExporter (Dialog)
6. Styling Approach & CSS Duplication Analysis
6.1 Source of Truth Conflicts
The demo has two competing styling systems that will collide when integrating into Astro:
A) Demo's Own System (src/index.css + Tailwind 4 + shadcn/tailwind.css)
- 175 lines of brand tokens copied from the Veristamp @veristamp/ui global.css (noted explicitly: /* Brand tokens copied from Veristamp @veristamp/ui global.css */).
- Defines --font-heading, --font-sans, --font-mono, --font-display via @theme inline.
- CSS variables --primary: #b34c35, --background: #f5f4ed (light) and dark variants.
- Uses shadcn/tailwind.css import for shadcn component base styles.
- Custom classes: .noise-overlay, .glass-header, .brand-wordmark.
- Imports: Tailwind 4, tw-animate-css, shadcn, Inter Variable, JetBrains Mono, Instrument Serif.
B) Target Astro Site's System
- @qql/ui-docs/global.css — 252 lines of -themed Starlight overrides.
- website/src/styles/global.css — imports Starlight Tailwind + @qql/ui-docs/global.css.
- website/src/styles/playground.css — 1028 lines of custom bare CSS for the Astro playground integration.
- Uses completely different variable names: --sl-color-accent, --sl-color-gray-1 through --sl-color-gray-6, --sl-color-bg, etc.
- No Tailwind 4 @theme — pure CSS custom properties.
- Fonts: Instrument Serif + IBM Plex Sans (different from demo's Inter Variable).
6.2 CSS Duplication Summary
Concern	Demo (index.css)	Astro site (ui-docs/global.css)	Astro site (playground.css)
Primary color	#b34c35 / #ba5442	#b34c35 / #ba5442 (same)	Uses --sl-color-accent
Background	#f5f4ed / #141413	#f5f4ed / #141413 (same)	Uses --sl-color-bg
Fonts	Inter Variable, JetBrains Mono, Instrument Serif	IBM Plex Sans, JetBrains Mono, Instrument Serif	Uses --sl-font / --sl-font-mono
Approach	Tailwind 4 + CSS variables	Pure CSS for Starlight overrides	Pure CSS grid/flex layouts
shadcn config	Enabled (shadcn/tailwind.css)	Not present	Not present
Component styles	Tailwind utility classes everywhere	Custom class names (.playground-page, .panel-heading, etc.)	Custom class names
The Astro site's playground.css reinvents the demo's entire layout (toolbars, panels, tabs, editor frame, dialog) as bare CSS — duplicating structurally what the React SPA does with Tailwind + react-resizable-panels.
6.3 shadcn/Base UI Components (18 files)
The demo has a full set of shadcn-style components built on @base-ui/react (Base UI, the successor to Radix). These are structurally complete:
- Button, Badge, Card, Dialog, Sheet, Tabs, Select, Tooltip, Separator, Input, Label, Textarea, Table, ScrollArea, Alert, DropdownMenu, Sonner, Resizable
- All use cva (class-variance-authority) for variant management.
- All use data-slot attributes (data-slot="button", etc.) for styling.
- Animation classes rely on tw-animate-css (animate-in, fade-in-0, zoom-in-95, etc.).
- These are not compatible with the Astro site's Starlight CSS components. Starlight uses its own tab/button/card styles. The demo's Base UI components would either need full re-theming to use --sl-* variables, or the Astro playground would need to embrace Tailwind 4 + tw-animate-css.
6.4 Visual Design Assessment
- Color palette: Warm terracotta primary (#b34c35 light, #ba5442 dark) on cream/charcoal background.
- Typography: Instrument Serif for headings, Inter Variable for UI, JetBrains Mono for code — editorial but modern.
- Glassmorphism: glass-header with backdrop-blur-xl, opacity: 78% background mix.
- Grain texture: .noise-overlay SVG mask for paper grain.
- Dark mode: Full dual palette in CSS variables, toggled via class="dark" + data-theme="dark".
- Spacing: Consistent px-2.5 sm:px-4 / py-1.5 pattern.
- Icon consistency: All UI icons from lucide-react.
6.5 Responsive Design
- Desktop (≥768px): Side-by-side ResizablePanelGroup (52% editor / 48% inspector) with draggable handle.
- Mobile (<768px): Stacked layout — editor on top, inspector on bottom with tab bar. No resizing.
- Header: Responsive hiding of nav links (hidden lg:flex), badges (hidden sm:inline-flex), keyboard hints (hidden sm:inline).
- Footer: Truncates Qdrant URL on small screens.
- PresetBrowser: Responsive grid (grid-cols-1 sm:grid-cols-2), sidebar collapses below sm.
- Settings dialog: sm:max-w-lg, HTTP fields use sm:grid-cols-2.
- Code exporter: sm:max-w-[900px], tabs at top.
- Policy Sheet: Full-width on mobile (w-full), sm:max-w-[520px].
6.6 Accessibility & Keyboard State
- ARIA labels: Most interactive elements have aria-label.
- Landmarks: <header aria-label="QQL site header">, <main aria-label="Workspace">, <footer aria-label="Status bar">, <section aria-label="Query editor">, <nav aria-label="Product">.
- Role attributes: role="toolbar" on scenario bar, role="alert" on error Alert.
- Focus visible: focus-visible:ring-2 focus-visible:ring-ring on all interactive elements (Tailwind ring pattern).
- Keyboard shortcuts: Ctrl/Cmd+Enter to execute; Ctrl/Cmd+K to open presets; d to toggle theme (via ThemeProvider).
- Shortcut guard: Theme toggle d key skips when focus is in input/textarea/contentEditable.
- Sonner toasts: Not currently wired into the app (no toast calls found in any component).
- Missing keyboard navigation: PresetBrowser item list is a flat set of button elements — no arrow-key navigation. Inspector tabs (Base UI Tabs) should support arrow navigation via Base UI natively.
- <noscript>: Only in the Astro site's EditorPanel.astro — the React SPA has no noscript fallback.
7. Assets & Dependencies
7.1 Production Build Output (dist/assets/ — 34 files)
Asset	Size hint	Role
index-*.js	~150-200 KB	Main React bundle (App, CM, all playground components, lucide icons)
transformers.web-*.js	~2-3 MB (lazy)	Code-split Transformers.js chunk
qql_wasm_bg-*.wasm	~1-2 MB	qql-wasm compiled WASM
ort-wasm-simd-threaded.asyncify-*.wasm	~2-3 MB	ONNX Runtime WASM for Transformers.js
index-*.css	~30-60 KB	All Tailwind + shadcn component styles
Font weights (woff/woff2)	~50-200 KB each	Inter Variable font files
Font weights (JetBrains Mono)	~30-80 KB each	400 + 500 weights
Font weights (Instrument Serif)	~20-50 KB each	400 weight
Total JS/WASM payload: ~6-10 MB (large — dominated by ONNX runtime + MiniLM model weights downloaded at runtime, not bundled).
7.2 Runtime Downloads (not bundled)
- MiniLM model weights from Hugging Face (~80 MB cached in browser cache on first use).
- Transformers.js is code-split (lazy import()).
7.3 Dependency Count
- dependencies: 24 packages (React 19, CM6, lucide, qql-wasm, transformers, shadcn, tailwind 4, base-ui, etc.)
- devDependencies: 12 packages (vite, typescript, eslint, prettier, etc.)
8. Integration Risks & Recommendations
8.1 What Should Be Retained (Proven Components)
These are well-tested, high-value components that should be ported (not copied-and-pasted due to framework/stack differences):
1. use-qql.ts hook — Core WASM lifecycle, analysis, execution, settings, client management. The most critical code to preserve.
2. lib/qql-language.ts — CodeMirror StreamLanguage for QQL. Framework-agnostic, directly reusable.
3. lib/editor-theme.ts — CM theme from CSS variables. Portable, will need --sl-color-* variable mapping.
4. lib/browser-embedder.ts — Lazy Transformers.js MiniLM pipeline. Framework-agnostic, directly reusable.
5. lib/qql-types.ts — All TypeScript types. Directly importable.
6. lib/presets.ts + data/presets.json — Preset catalog. JSON data plus helpers are reusable.
7. lib/site.ts — URL config (needs adaptation for Astro routing).
8. Preset validation script (scripts/validate-presets.mjs) — Already works standalone.
8.2 What Must Be Rebuilt (Not Reusable as-is)
1. All src/components/ui/* (18 shadcn/Base UI files) — Built on @base-ui/react with Tailwind 4 + tw-animate-css. The Astro site uses Starlight's CSS + @qql/ui-docs/global.css without React. A new set of HTML/CSS-only components (or a lightweight web component / Alpine.js approach) is required.
2. App.tsx — 913-line monolith of React state + layout. Must be decomposed into Astro-compatible patterns.
3. All playground feature components — They use React hooks, Tailwind classes, lucide-react icons, and Base UI. The Astro site's existing playground.css already duplicates much of this with bare CSS (1028 lines). The React SPA's Tailwind approach and the Astro site's CSS approach must be reconciled.
8.3 Integration Risks
Risk	Severity	Details
Stack mismatch	High	React 19 + Base UI + Tailwind 4 vs Astro 7 + Starlight + bare CSS. Cannot directly embed React components into Astro pages without client-side hydration overhead. An astro/react integration is possible but adds complexity.
CSS token duplication	High	Demo uses --primary, --foreground, etc.; Astro site uses --sl-color-accent, --sl-color-gray-*. Both encode the same brand colors but through different variable names. playground.css (1028 lines) duplicates what the React SPA does with Tailwind.
Font mismatch	Medium	Demo uses Inter Variable; Astro uses IBM Plex Sans. Both use Instrument Serif + JetBrains Mono.
WASM threading	Medium	COOP/COEP headers already set in _headers and Vite config. Astro site must ensure these headers for any page hosting the playground.
TypeScript version	Medium	Demo = TS 6 (erasableSyntaxOnly); Astro = TS 5.9. Direct imports may fail.
Vite version mismatch	Medium	Demo = Vite 8; Astro 7 = Vite 6.x. Shared WASM handling may behave differently.
Animation classes	Low	Demo uses tw-animate-css. Equivalent Starlight animations would need custom CSS or dropping animations.
Font weight duplication	Low	Both load JetBrains Mono and Instrument Serif — could share across the Astro site.
Dependency weight	Low	Moving React 19 + 24 deps into an Astro page adds significant JS payload. Consider whether the playground needs to be an SPA or if a lighter interaction model suffices.
lucide-react vs inline SVG	Low	Demo uses lucide-react icons. Astro playground uses inline SVG paths. Must pick one approach.
8.4 Recommended Migration Strategy
Recommended approach: Port the useQql hook and TypeScript libs into the Astro site, then build new Astro-native components.
1. Phase 1 — Core library migration: Copy src/lib/* (qql-types, qql-language, editor-theme, browser-embedder, presets, site, utils) into the Astro site (e.g., website/src/playground-lib/). Reconcile TypeScript targets (TS 5.9 compatible).
2. Phase 2 — WASM integration: Copy use-qql.ts as a standalone TypeScript module. It has zero React dependency — the WASM Client + analyze() are plain functions. Re-export as a plain EventEmitter or observable pattern.
3. Phase 3 — UI rebuild: Replace the demo's 18 Base UI components and 12 playground feature components with Astro-friendly equivalents:
- Use Starlight's built-in component overrides for common patterns.
- For interactive elements (editor, inspector tabs, results), use vanilla CodeMirror + DOM manipulation scripted in <script> tags or a minimal client-side framework.
- Adopt the existing playground.css as the base, refining rather than rewriting.
4. Phase 4 — Token alignment: Map --sl-color-* to the actual brand values already in @qql/ui-docs/global.css. Eliminate playground.css variable duplication by using --sl-color-accent, --sl-color-gray-*, etc. consistently.
5. Phase 5 — Remove duplication: Delete or deprecate playground.css sections that duplicate what Tailwind could provide, or vice versa. Pick one approach (recommended: stick with the Astro site's existing CSS approach to avoid adding Tailwind + shadcn to the docs build).
6. Phase 6 — Test & CI: Port validate-presets.mjs as-is. The qql-wasm npm dependency is identical.
8.5 Alternative: Keep the SPA and embed it
Instead of porting components, keep the React SPA as a standalone build and embed it in the Astro site via an <iframe> or a reverse-proxied sub-path (current approach with /playground/). This reduces integration risk to zero but:
- Prevents tight integration with docs navigation (theme switching, shared header/footer).
- Adds an extra page load for users.
- Cannot share CodeMirror/QQL tokens with docs code examples.
Given the audit findings, porting is recommended if tight integration is desired; the <iframe> path is safer but less cohesive.
9. Significant File Paths (Summary)
Demo (source of truth for porting)
- /data/codebases/qql-wasm-demo/src/hooks/use-qql.ts — Core WASM hook
- /data/codebases/qql-wasm-demo/src/lib/qql-types.ts — All TypeScript types
- /data/codebases/qql-wasm-demo/src/lib/qql-language.ts — CM StreamLanguage
- /data/codebases/qql-wasm-demo/src/lib/editor-theme.ts — CM themes
- /data/codebases/qql-wasm-demo/src/lib/browser-embedder.ts — MiniLM pipeline
- /data/codebases/qql-wasm-demo/src/lib/presets.ts — Preset helpers
- /data/codebases/qql-wasm-demo/src/data/presets.json — Preset catalog
- /data/codebases/qql-wasm-demo/src/lib/site.ts — Site URL config
- /data/codebases/qql-wasm-demo/scripts/validate-presets.mjs — CI validator
- /data/codebases/qql-wasm-demo/src/index.css — Brand tokens (must reconcile)
- /data/codebases/qql-wasm-demo/vite.config.ts — WASM build config
Astro target (files to align with)
- /data/codebases/qql-rs/website/src/styles/playground.css — Existing playground CSS (1028 lines, needs dedup)
- /data/codebases/qql-rs/website/src/styles/global.css — Site CSS entry point
- /data/codebases/qql-rs/website/packages/ui-docs/src/styles/global.css —  theme tokens
- /data/codebases/qql-rs/website/src/components/playground/ — 6 Astro scaffold components
- /data/codebases/qql-rs/website/astro.config.mjs — Astro config with WASM alias
10. Concise Migration Recommendation
Port the core WASM/TypeScript logic (use-qql.ts, all lib/, data/, and scripts/) into the Astro site as plain TypeScript modules. Rebuild the UI layer using the existing playground.css + Starlight components + vanilla CodeMirror DOM scripting, instead of porting React + Base UI. Eliminate the 1028-line playground.css → Starlight variable duplication by choosing one styling system (recommend: pure CSS with --sl-* variables, no Tailwind in the playground). The qql-wasm npm package works identically in both environments — no WASM build changes needed. Keep the SPA as a /playground/ sub-path fallback if rapid integration is prioritized over deep UI cohesion.
This report covers all requested areas. The key tensions are: (1) React vs Astro component model, (2) Tailwind 4 vs bare CSS approach, and (3) Base UI vs Starlight component semantics. The WASM integration layer is solid and portable.
