# Playground — agent guide

Interactive QQL bench: CodeMirror + qql-wasm + a live Qdrant. The route is
`src/pages/playground/index.astro`, the markup is
`src/components/playground/*.astro`, and all behavior lives here.

```
playground.ts   entry — mounts and starts
bootstrap.ts    composition root: initial document, editor, wiring
core/           store · types · dom · wasm bridge · refresh hub
editor/         CodeMirror · language · keywords.generated · statements · analysis · selection · editor-actions
run/            run targets · execution · per-statement results · run menu
panels/         inspector · status-bar · statement-nav · render (response payloads)
dialogs/        presets · settings · policy · export
shell/          layout · palette · commands · shortcuts · delegates
services/       client · connection · embedder · codegen (SDK snippets)
```

Imports point downward only. `bootstrap.ts` is the composition root; the one
other cross-layer call is `run/run.ts` → `shell/layout#applyMobileView`, which
reveals the result pane after a run on stacked (mobile) layouts. Panels and
dialogs never import each other.

## State

- `core/store.ts` holds the single state object and its persistence. Mutate
  `state.*`, then call the matching `refresh*()` from `core/refresh.ts`.
- Never read playground state back out of the DOM, and never keep a second
  copy of a value that is already in `state`.
- Persisted keys (localStorage): `qql-playground.settings.v1`, `.policy.v2`,
  `.run.v1`, `.inspector-tab.v1`, `.wrap.v1`, `.mobile-view.v1`, `.split.v1`,
  `.workspace.v1`. Values are written JSON-encoded; `readStored` still accepts
  legacy unquoted strings and `readStoredMerged` ignores non-object entries.
  New persisted fields go through `core/types.ts` + `core/store.ts` with
  defaults and a version bump — do not mutate an old key in place.
- Swapping documents goes through `editor/analysis.ts#loadSource`: it clears
  the previous run (`resetRunState`), resets the caret, and focuses the editor
  (loading from a modal would otherwise let the browser's focus restoration
  hand CodeMirror a stale DOM selection).
- A runtime error (`state.executionError`) stays visible while you edit and is
  cleared by the next run or document load — do not clear it on keystrokes.

## DOM contract

Markup and modules talk exclusively through `data-*` attributes; components
carry no state. Grep before renaming:

- **Run:** `[data-run]`, `[data-run-label]`, `[data-run-menu-toggle]`,
  `[data-run-menu]`, `[data-run-mode]`, `[data-run-stop-on-error]`,
  `[data-run-statement-hint]`
- **Statements:** `[data-statement-rail]`, `[data-stmt-chip]`,
  `[data-statement-pos]`, `[data-statement-select]`, `[data-statement-wrap]`,
  `[data-stmt-prev]`, `[data-stmt-next]`, `[data-select-stmt]`,
  `[data-cursor-pos]`
- **Inspector:** `[data-inspector-tab]`, `[data-inspector-panel]`,
  `[data-output="wire|ast|tokens|explain|metrics|response"]`,
  `[data-response-rich]`, `[data-response-context*]`, `[data-hit-count-badge]`,
  `[data-plan-card]`, `[data-empty-plan]`, `[data-routes-list]`
- **Header/status:** `[data-status-endpoint]`, `[data-status-wasm]`,
  `[data-status-embed]`, `[data-embed-dim-chip]`, `[data-connected-dot]`,
  `[data-validation-badge]`, `[data-analysis-summary]`, `[data-stmt-count]`,
  `[data-policy-dot]`, `[data-policy-chip]`
- **Dialogs:** `[data-settings-form]`, `[data-policy-form]`,
  `[data-policy-recipe]`, `[data-preset-query]`, `[data-preset-results]`,
  `[data-export-tab]`, `[data-export-output]`, `[data-export-echo]`,
  `[data-palette-input]`, `[data-palette-list]`, `[data-close-dialog]`,
  `[data-open-*]`

**Visibility rule:** JS toggles the `hidden` *attribute* (`element.hidden = …`).
An element must not also carry Tailwind's `hidden` *class* unless a responsive
variant (`sm:block`) is meant to reveal it from that breakpoint on — otherwise
`hidden = false` leaves it invisible. That exact mix hid the hit-count badge
and the response context strip until it was caught.

## Run engine (`run/run.ts`)

- Target resolution for ⌘↵: **selection → statement under the caret → the
  exact line**. Broken or comment-only documents still run something.
- Modes: `smart`, `statement`, `all`, `from`. The run menu, the palette
  (including "Run selected statement"), and the keyboard all funnel into
  `runMode` / `runSmart` / `runAll`.
- Every target carries a `startOffset` (char offset of its text in the
  document). Executor error spans are relative to the *executed text*, so both
  `buildFailure` and `failingStatementIndex` must shift by it — otherwise a
  slice run blames the wrong statement and the lint marker lands on the wrong
  line.
- `state.runPrefs.stopOnError` maps to the executor's
  `onError: "stop" | "continue"`. A stopped multi-statement run throws; a
  continued one reports per-statement `ok: false` results in-band. Both paths
  must end in `applyRunReport` / `applyRunFailure`, and both must leave the
  inspector on the statement that actually failed.
- Results map to statements through `state.executedStatements` (execution
  order) — never by index guessing. `store#resultForStatement` falls back to
  the single result when a raw line/selection run maps to no statement.
- Tenant policy fails closed on multi-statement runs (one statement at a
  time); keep that guard when touching target resolution.
- A run selects the response tab and, on stacked layouts, switches the pane to
  Result so hits/errors are visible without a manual toggle.

## Editor (`editor/`)

- `editor/editor.ts` builds CodeMirror. Two ordering rules: the custom keymap
  must come **before** `basicSetup` (its defaultKeymap binds `Mod-Enter` to
  `insertBlankLine` and would swallow Run), and `qqlCommentTokens` must stay in
  the extension list (`toggleComment` no-ops without `commentTokens`).
- `statementSpansField` recomputes statement spans on document change only and
  feeds the rail, the gutter, selection, and run targeting.
- Gutter markers must return **fresh DOM** from `toDOM()`. CodeMirror may
  re-insert the same marker instance into a rebuilt gutter element; a cached
  node crashes the gutter plugin.
- `editor/statements.ts` is a text-level scanner that mirrors the lexer's
  trivia rules and splits on top-level `;` only — strings (including raw and
  triple-quoted), `--` comments, and `BATCH { … }` nesting all protect a
  semicolon. Its statement counts must match
  `language/v1/fixtures/expected/*.json`; when you change it, re-check against
  that corpus (`pnpm validate:qql` verifies the parser, not this scanner).
- `queueAnalysis` (immediate spans + rail, 80 ms debounced parse and
  localStorage write) is for edits; `runAnalysis` is the synchronous full
  pass. Do not call `runAnalysis` per keystroke.

## Adding a feature

1. **State** — field in `core/types.ts` + `core/store.ts` (persisted? version
   the key).
2. **Render** — a `render*` function in the owning panel, invoked from
   `core/refresh.ts`.
3. **Markup** — `data-*` hooks in `src/components/playground/*.astro`.
4. **Action** — a function in the owning module, then expose it through
   `shell/commands.ts` (palette) and, if it has a shortcut, the editor keymap
   or `shell/shortcuts.ts`.
5. **Verify** — browser pass + `pnpm check`.

## Verification

`pnpm check` covers build, types, lint, and docs. Behavior needs a browser
pass (`pnpm dev` → `/playground/`):

- multi-statement script: rail chips, gutter badges, and the inspector select
  agree with the caret; ⌘↵ runs the caret statement; ⇧⌘↵ runs all; statuses
  show ✓ / ✕ / –; the response context strip names the right statement
- a failing statement shows an error card + inline lint marker on the failing
  line (test a *slice* run, e.g. select statements 2–3 with the error in 3,
  and check the status lands on 3) and does not mark earlier statements failed
- ⌘/ toggles comments, Alt+Shift+F formats without moving the caret, ⌘K
  palette filters and runs
- run menu (all four targets; stop-on-error persists across reload), share
  link, wrap toggle, mobile Editor/Result switch (a run auto-reveals Result)
- dialogs: export closes via button and backdrop; settings/policy reopen with
  persisted values; loading an example resets the caret to Ln 1, Col 1
- reload once and confirm tab, wrap, mobile pane, and stop-on-error survived
- zero console errors, both themes, narrow viewport
