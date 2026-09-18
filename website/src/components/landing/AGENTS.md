# Landing page — agent guide

The `/` marketing page. `src/pages/index.astro` composes these components with
copy from `src/data/landing.ts` and site identity from `src/config/site.ts`.
Site chrome (`SiteHeader`, `SiteFooter`) and `FaqList` live one level up.

| File | Owns |
|---|---|
| `src/pages/index.astro` | Route, head/SEO/JSON-LD, verified QQL specimens, section order |
| `LandingHero.astro` | Headline, CTAs, specimen code card, stats |
| `ProblemCompare.astro` | QQL vs REST JSON / SDK comparator |
| `PipelineSteps.astro` | Parse → Validate → Plan → Dispatch narrative |
| `LanguageSamples.astro` | Query-form samples |
| `InstallGrid.astro` | Per-runtime install commands |
| `CtaBand.astro` | Closing CTA band |
| `src/data/landing.ts` | All copy, stats, FAQ items, install commands |
| `src/components/FaqList.astro` | FAQ accordion (reads `landing.ts` directly) |

Components are presentation only and take props; the page passes the copy.
`FaqList` is the one component that imports `landing.ts` itself.

## Rules

1. **Copy changes go in `src/data/landing.ts`** — except the `verifiedQql*`
   specimens, which live in `index.astro` because the docs verifier scans
   `src/pages`.
2. **Specimens must be verified QQL.** The `String.raw` marker is load-bearing:
   `scripts/verify-qql-docs.mjs` parses every `verifiedQql*` template. Do not
   change the marker, do not add unverified syntax, and run `pnpm validate:qql`
   after editing.
3. **Numbers need evidence.** `stats`, `wasmNote`, and FAQ claims about sizes
   or counts are measured from the repo (WASM bytes from
   `website/.wasm/qql-wasm/qql_wasm_bg.wasm`, conformance counts from the
   fixture corpus). Update the comment next to a number when you update it.
4. **Design stays on tokens.** No new colors, gradients, or CSS files — use the
   `styles.css` primitives (`premium-card`, `code-card`, `mono-label`, `btn`)
   plus Tailwind utilities. Global reduced-motion handling already exists; do
   not add motion that ignores it.
5. **The comparator is the pitch.** Native panes are deliberately truncated —
   never inline real vectors or let a pane exceed ~28 lines; the previous
   300-float inlining caused a 416px layout shift.

## Verify

- `pnpm validate:qql` (specimens) and `pnpm check` (budget, build, anchors).
- Browser: `/` at desktop and mobile, light and dark, including a small-laptop
  viewport; the hero must stay above the fold and not shift while code blocks
  hydrate.
