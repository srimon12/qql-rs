# QQL Website — Brand & Visual Direction (landing + playground)

**Scope:** ground-up redesign of `/` and `/playground/`, with docs inheriting the same tokens.
**Implementation target:** ONE universal CSS file ≤500 lines (tokens + primitives), loaded by landing, playground, and Starlight (`customCss`); page-specific composition via Tailwind utilities in markup. Suggested home: `website/packages/ui-docs/src/styles/qql.css`. Delete `landing.css` (1,453 lines) and the `global.css`/`chrome.css` override piles; playground-local CSS keeps only CodeMirror's third-party DOM (≤60 lines).
**Canonical surface:** dark is the product's home (docs, playground, marketing default); light is a fully designed paper twin, not an inversion. Both themes ship with parity.
**Reference:** current baseline screenshots in `/tmp/opencode/qql-redesign/baseline/`; mark at `website/public/logo.svg`.

---

## 1. Positioning & message architecture

**One-liner.** QQL is a declarative query language for Qdrant: one SQL-like statement for vector search, filters, and schema — compiled to REST, gRPC, or in-process edge from Rust, Python, Node.js, WASM, and the CLI.

**The one idea the landing must land in 5 seconds:** *"I can replace my Qdrant JSON/SDK boilerplate with readable SQL — and the same statement runs in every runtime I ship."* Language first (it is a language, not a wrapper), portability second. Every hero element serves one of those two; anything serving neither is cut.

**Hero headline options.**
1. **"SQL for Qdrant."** (recommended) — three words: category + analogy, zero decode cost, already the line shared by the README and the Veristamp site, so recognition compounds. Risk: deadpan — mitigated by the subhead and specimen carrying the proof.
2. **"One language. Every runtime."** — leads with the moat (one source across Rust/Python/Node/WASM/CLI/edge) and doesn't require knowing Postgres-vs-Qdrant. Loses the category anchor; only usable if the subhead says "declarative queries for Qdrant".
3. **"Cut the JSON."** — pain-first, echoes the mark's slit and the compare section. Strongest hook for people already fighting payloads; too negative as the first line a newcomer reads. Use as the OG/campaign line, never the `<h1>`.

**Supporting line (hero lede, recommended):** "QQL is to Qdrant what SQL is to Postgres. One statement for hybrid search, filters, and schema — the same source in Rust, Python, Node, WASM, and the CLI." If headline B is chosen: "Declarative queries for Qdrant, from Rust to the browser."

**CTAs.** Primary: **"Try the playground"** → `/playground/` (runs the real WASM parser in-browser — no cluster, no signup; the proof is one click away). Secondary: **"Quickstart"** → `/docs/getting-started/quickstart/`. Micro-CTA directly under the buttons: one copyable mono install line (`curl … | sh`, `pip install pyqql`, `npm i @veristamp/nqql` — CLI default). Exactly two buttons; "GitHub" lives in the header icon only.
Eyebrow (kept, above the headline): `● Open source · MIT · v0.4.0` — the ping-dot appears exactly once on the whole site, here.

**Proof order down the page:** it's a real language (compare) → here's why (pipeline) → it covers my case (language samples) → it installs in my stack (runtimes) → try it now (CTA).

**Voice rules.** Second person, present tense. Numbers carry provenance ("7 lines vs 36", "276 conformance statements", "1.9 MB WASM"). State capability, not belief. Sentence case for titles except QQL keywords. A period instead of an exclamation mark — always.

---

## 2. Personality

1. **Declarative** — this: sentences that state a capability and stop ("One statement. Every runtime."); not that: stacked adjectives ("fast, flexible, powerful").
2. **Plainspoken** — this: concrete nouns (parser, plan, route, span) and counted lines; not that: metaphor ("superpowers for your vectors") or hype ("blazingly fast").
3. **Instrumented** — this: exact codes (`QQL-PARSE-014`), real latencies, honest versioning ("v0.4.0 — stabilizing, not frozen"); not that: rounded-up claims and decorative chrome.
4. **Paper-warm** — this: cream, ink, one terracotta; a serif that reads like a printed manual; not that: cold blue-gray SaaS, neon cyberpunk, pastel candy.
5. **Composed** — this: generous whitespace, one accent, quiet hovers, content sets the rhythm; not that: shouting CTAs, animated counters, competing gradients.

**Anti-references — what QQL must never look like.** An "AI product": violet/indigo gradient meshes, glassmorphism cards, floating gradient blobs, glow orbs, badge soup, emoji icons, gradient headline text, stock 3D renders, "Trusted by" logo walls, tilted floating dashboard mockups, typewriters and count-ups. Nor generic Tailwind: blue-500 primary, `rounded-2xl shadow-lg` on everything, centered hero + three feature cards. The reference texture is **editorial print + a precision instrument**: hairlines, mono labels, tabular numbers, real code, real state.

---

## 3. Typography

**Stack (keep — no new families).** Sans: Geist Variable. Serif: Newsreader (300/400 only). Mono: Geist Mono Variable.

**Roles.**
- **Serif** = editorial voice: landing hero, section titles (h1/h2), docs article titles, pull quotes. Never in the playground, never below 24px, never bold.
- **Sans** = everything readable: body, cards, docs prose, playground UI, buttons.
- **Mono** = machine voice: code, file names, status, chips, tokens, numbers, version tags, and the **QQL wordmark** (Geist Mono 600, +0.02em). The wordmark switch from serif to mono is the main typographic differentiator from Veristamp; the mark itself is unchanged.

**Scale** (mobile values via `clamp()`; tracking in em):

| Role | Family / weight | Size / line-height / tracking |
|---|---|---|
| Hero h1 | Newsreader 300 | clamp(2.5 → 4.5rem) / 1.02 / −0.03; `text-wrap: balance` |
| Section h2 | Newsreader 300 | clamp(1.75 → 2.5rem) / 1.1 / −0.02 |
| Card h3 | Geist 600 | 1.25rem / 1.3 / −0.01 |
| Lede | Geist 400 | 1.125rem / 1.6; max-width 34rem |
| Body | Geist 400 | 1rem / 1.65; `text-wrap: pretty` |
| UI small | Geist 500 | 0.875rem / 1.5 |
| Mono label | Geist Mono 600 | 0.6875rem / 1.2, uppercase, +0.12 |
| Mono data | Geist Mono 400–500 | 0.75rem / 1.6, `tabular-nums` |
| Code | Geist Mono 400 | 0.8125rem / 1.7 (docs 0.85rem) |

**Rules.** Uppercase only for labels ≤3 words. Mono never sets sentences. Hero headline ≤9 words / ≤2 lines at 3.5rem. No letter-spacing beyond −0.03em on serif. Links use `--accent-text`; underlines appear on hover/focus only.

---

## 4. Color system

One token layer. Keep the `--sl-color-*` names — Starlight consumes them, so the docs inherit the system for free — and add `--q-*` only where Starlight has no slot (syntax, status, layout, texture). Terracotta stays but is demoted from decoration to **signal**: accent = action/state, never paragraph color.

**Dark (canonical).**

| Token | Value | Use |
|---|---|---|
| `--sl-color-bg` | `#141413` | page |
| `--sl-color-gray-6` | `#1c1b19` | surface: panels, cards, code |
| `--sl-color-gray-7` | `#232220` | raised: hover, active rows |
| `--sl-color-gray-5` | `#2b2a26` | hairline borders |
| `--sl-color-gray-4` | `#5c5a52` | line numbers, disabled — decorative only |
| `--sl-color-gray-3` | `#8b887e` | muted labels (5.3:1 ✓) |
| `--sl-color-gray-2` | `#b6b3aa` | secondary text |
| `--sl-color-gray-1` / `text` | `#e9e7e1` | body |
| `--sl-color-white` | `#f6f4ee` | headings, emphasis |
| `--sl-color-accent` | `#ba5442` | fills, focus rings, the 1px slit |
| `--sl-color-accent-high` / `text` | `#d29084` | links, small accent text (7.2:1 ✓) |
| `--sl-color-accent-low` | `#2b140d` | 6–8% tint base |

**Light (peer theme).**

| Token | Value | Use |
|---|---|---|
| `--sl-color-bg` | `#f5f4ed` | page |
| `--sl-color-gray-6` | `#fbfaf5` | surface |
| `--sl-color-gray-7` | `#efede2` | raised |
| `--sl-color-gray-5` | `#dfdcd0` | hairline borders |
| `--sl-color-gray-4` | `#a9a698` | decorative only |
| `--sl-color-gray-3` | `#6e6c63` | muted labels (4.8:1 ✓) |
| `--sl-color-gray-2` | `#4c4a44` | secondary text |
| `--sl-color-gray-1` / `text` | `#1e1d1a` | body |
| `--sl-color-white` | `#141413` | headings |
| `--sl-color-accent` | `#b34c35` | fills, focus |
| `--sl-color-accent-high` / `text` | `#8d3a29` | links, small accent text (6.9:1 ✓) |
| `--sl-color-accent-low` | `#f0e6e0` | tint |

**Code / syntax** (`--q-sx-*`; code only, never UI text):

| Role | Dark | Light |
|---|---|---|
| keyword | `#ffab98` | `#9c3d2b` |
| operator | `#d29084` | `#a8503c` |
| string | `#9fe3b8` | `#166b48` |
| number | `#f2c983` | `#7a5100` |
| name / identifier | `#dedbd3` | `#33322e` |
| comment / punctuation | `#8b8880` | `#6f6d64` |

**Status** (`--q-ok/warn/bad`): success `#46b17c` / `#207d52`; warning `#d9a53f` / `#8a6100`; error `#dd675a` / `#a94035` — dot + text pairs only, never background floods.

**Contrast notes (the ones that decide usage).**
- `#ba5442` on dark bg = **4.0:1** → large text and UI strokes only; any accent text under 18px uses `--accent-text` `#d29084` (7.2:1).
- `#b34c35` on cream = **4.8:1** → passes ≥14px; small links use `#8d3a29` (6.9:1).
- White on `#ba5442` = **4.7:1** → button labels at 600 weight ≥14px; never 12px chip text.
- `gray-4` is ≈2.7:1 → decorative only.
- Tinted surfaces (`color-mix(accent 6–8%, surface)`) keep `gray-1` text; never place `gray-3` on a tint.

---

## 5. Layout & rhythm

**Widths.** `--q-page: min(100% - 2rem, 72rem)` for content; `--q-wide: 88rem` for header/footer and the one bleed. Docs keep Starlight's content column and sidebars.

**Spacing.** Sections are separated by 1px hairlines and rhythm, not boxes: padding `clamp(4rem, 8vw, 6.5rem)`. Scale: 4 / 8 / 12 / 16 / 24 / 40 / 64 / 96px. Cards pad 20–24px; panels 12–16px.

**The hero — must fit a 1280×800 laptop with CTAs visible without scrolling.** One column below 64rem; at ≥64rem, copy left (max 34rem) + specimen right (max 30rem), gap 3.5rem, hero total ≤620px. Eyebrow ≤40px tall; headline ≤2 lines at 3.5rem; action buttons 48px. The specimen is a real `search.qql` statement (6–8 lines) with a compiled footer line: `POST /collections/docs/points/query · 200 · 1.2ms`. The hero shows *language → dispatch*; the JSON wall belongs to the compare section.

**Code specimen framing (one component used everywhere).** `figure` → header strip 36px (file name mono 12px left; meta mono 11px `gray-3` right) → code body on `--surface` (padding 16/20, optional gutter, suppressed scrollbar chrome) → footer strip 32px with real state (route, latency, engine, "WASM · in-browser"). Radius **10px**, 1px border, **no shadow at rest**; hover shifts only the border (`color-mix(accent 35%, border)`). No traffic-light dots — the header's left edge carries a 1px × 12px accent **slit** instead: the mark's device, reused as a file-tab tick.

**Grid breaks (exactly two, so they remain events).**
1. **Hero specimen bleed** — ≥80rem, the specimen figure extends past the 72rem text column to the right edge of the 88rem field (negative margin): the code crosses the page's own margin.
2. **Closing CTA band** — full-bleed inverted band: light theme = ink `#141413` band with cream text; dark theme = `color-mix(accent 8%, #1c1b19)` band with a 2px accent top rule. Inner content returns to `--q-page`.

**CSS budget** (target ≈430 lines): tokens 110–130 · base + typography + focus 80–90 · primitives 150–170 (btn, chip, card, code-card, section, mono-label, field, tab, toast) · texture + motion 40–50. Everything else — grids, section composition, responsive arrangement — Tailwind utilities in markup.

---

## 6. Texture & motion

**Grain:** one fixed layer per landing page, opacity 0.025, 16px dot mask, `pointer-events: none`, below content. Never in docs or playground (text clarity + paint cost). **Hairline grid:** hero only, 24px, radial mask from the top, opacity ≤0.5. One texture moment per page; no gradients anywhere else.

**Motion — exactly one mechanism.** Elements marked `.reveal` start `opacity: 0; translateY(12px)`; a single IntersectionObserver (`threshold: 0.1`, `rootMargin: 0 0 -10% 0`, once) adds `.is-in` → 420ms `cubic-bezier(.16, 1, .3, 1)` to neutral. Stagger with `--d: calc(40ms * var(--i))`, capped at 3 steps. Reveal section heads and card rows only — never code text, never above-the-fold content. Hover = border/color change, 160ms; lift `translateY(-1px)` on interactive cards only; `:active` scale .98 on buttons. Focus: 2px `--accent` outline, offset 2px, on every interactive element, never removed. **Reduced motion:** reveals resolve instantly (no transition/transform), no ping, no smooth scroll. No parallax, count-ups, marquees, or scroll-jacking anywhere.

---

## 7. Landing narrative (6 sections, in order)

| # | Section | Verdict | Rationale |
|---|---|---|---|
| 1 | **Hero** — "SQL for Qdrant." | keep, rewrite | Same headline; new specimen (statement → compiled route) + one micro install line; drop window dots and the hero's install-command bar. |
| 2 | **Compare** — "Same query. No boilerplate." | keep, promote to #2, tighten | The strongest 10-second proof (7 lines vs 36). Remove the pane-balance slider; end with a 3-number mono ruler — 12 query forms · 276 conformance statements · 6 runtimes. |
| 3 | **Pipeline** — "How a statement runs." | keep, compress | Parse → Validate → Plan → Dispatch as one horizontal hairline rail with mono step numbers, not four cards. Pre-loads the playground's Plan/Wire tabs. |
| 4 | **Language** — "One grammar. The whole surface." | keep, cut to 3 specimens | Hybrid, Formula, Facet only (grouped/recommend live in docs). Each specimen links to the playground with `?q=…` — the strongest conversion path. |
| 5 | **Runtimes** — "One statement. Every runtime." | rewrite of InstallGrid + StatsStrip | Merge runtime story and install into one 3×2 grid (CLI, Python, Node, Rust, WASM, VS Code): name + copyable command + docs link, the only dense grid on the page. "1.9 MB WASM" becomes a footnote on the WASM cell. Cut the standalone stats strip. |
| 6 | **CTA band** — "Run a query in the browser." | keep, rewrite as ink band | End on the action: playground primary, docs secondary; mono footnote "no cluster · no signup · MIT". |

**Cut:** FAQ as a landing section — move the six Q&As to `/docs/faq/` with the FAQPage JSON-LD there; a landing page should end on an action, not a questionnaire. If SEO needs FAQ presence on `/`, keep a compact 3-item `<details>` block inside section 5 and no more.

---

## 8. Playground direction

**Concept: a bench, not a page.** One viewport, no marketing. Three surfaces with strict hierarchy: the editor (largest), the inspector (tabs), the run state (one 28px status line).

**Layout.** Below the 64px chrome: toolbar 44px → work area (fills viewport) → status bar 28px. Desktop: editor | 8px draggable divider | inspector, default split 54/46. Editor: Geist Mono 13px/1.7, line-number gutter, active-line tint `accent 4%`, selection `accent 22%`, no minimap, wrap off. Inspector tabs in mono 11px with a 2px accent underline on the active tab: Plan · Wire · AST · Explain · Tokens · Metrics · Result. Panels never remount on tab switch; long lists use `content-visibility: auto`.

**Toolbar = 4 controls max:** runtime status dot + "WASM ready" (live), Example picker, Connection, Docs link. Merge the current "Offline analyze" and "Connected" pills into one status chip whose dot and tooltip carry the state — this kills the badge-soup row. Telemetry that isn't actionable (endpoint, embeddings, engine, parse/plan ms, token count) moves to the status bar.

**Tone of states.** Engineer's voice, exact, no mascots:
- Empty: "Paste a statement or load an example. Parsing runs locally — nothing leaves this tab."
- Error: code + caret + plain sentence + next step: `QQL-PARSE-014 · line 3, col 14 · expected FROM after the query vector · open the language reference`. The error code links to docs; the span highlights in the editor.
- Success: quiet — a mono footer line (`parsed 0.4ms · planned 0.9ms`), never a toast storm.
- Backend unavailable: "No Qdrant at localhost:6333. Offline analysis still works." State the fallback; don't apologize.

**Brand inheritance without slowing the tool.** Same tokens and hairlines; panel radius 8px (not 10/16); accent only for focus, active tab, split-handle hover, and status dots; no serif, no grain, no reveal animations, no backdrop blur outside the sticky header; transitions ≤160ms and none on content re-render. Load two font files (sans + mono variable); Newsreader is dropped from the playground page. The only decoration is the same 1px accent slit used as the active-tab rule.

---

## 9. Do-not list

1. Purple/violet/indigo gradients or "AI glow" — one terracotta accent on flat paper/ink.
2. Glassmorphism (`backdrop-blur` cards) — tinted surface + 1px hairline; blur only in the fixed header.
3. macOS traffic-light dots on code cards — file-tab label + the 12px accent slit tick.
4. Floating blobs, mesh/aurora backgrounds, particles — one grain layer + one masked hairline grid.
5. Badge soup (4+ chips per card, tech-logo walls) — max one status chip; everything else a text list.
6. Emoji as UI icons (⚡ ✅ 🚀) — 16px stroke SVG glyphs or mono ASCII.
7. Gradient text or highlight-pen underlines on headings — solid ink; accent for links only.
8. Animated counters, typewriter headlines, logo marquees — static numbers with provenance.
9. Centered hero + three-feature-card row (generic SaaS) — asymmetric two-column hero with a live specimen.
10. Fake social proof (logo walls, invented testimonials, rounded star counts) — real numbers and links, or nothing.
11. Tailwind defaults doing the branding (`rounded-2xl shadow-lg`, blue-500, uniform `gap-4`) — 8–10px radii, borders over shadows, the spacing scale above.
12. Neon-on-black cyberpunk or stock 3D/isometric illustration — warm paper dark, real screenshots, real diagrams.
