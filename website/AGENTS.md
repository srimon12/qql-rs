# Website — agent guide

Astro 7 + Starlight + Tailwind 4 site for QQL. One app, three surfaces:

| Route | Surface | Area guide |
|---|---|---|
| `/` | Landing page | `src/components/landing/AGENTS.md` |
| `/docs/` | Starlight documentation | `src/content/AGENTS.md` |
| `/playground/` | Interactive QQL playground | `src/lib/playground/AGENTS.md` |

Everything runs from this directory. There is no root `package.json` and no
pnpm workspace — all commands below are executed in `website/`.

## Commands

| Command | What it does |
|---|---|
| `pnpm dev` | Builds the current-checkout WASM, then serves on `:4321` |
| `pnpm check` | The CI gate: CSS budget → QQL docs validation → WASM build → OG images → `astro build` → anchor check → headers check |
| `pnpm typecheck` | `astro check` — must be **0 errors / 0 warnings / 0 hints** |
| `pnpm lint` / `pnpm format` | Biome check / check `--write` |
| `pnpm validate:qql` | Parses every docs example and fixture with a freshly built Node WASM SDK |
| `pnpm check:css` | The authored-CSS budget |

Run `pnpm check` before calling a site change done. Visual work also needs a
browser pass (`pnpm dev`) — the gates cannot see layout.

## Layout

| Path | Owns |
|---|---|
| `src/pages/` | Routes: `index.astro` (landing), `playground/index.astro`, `robots.txt.ts` |
| `src/components/` | Site chrome (`SiteHeader`, `SiteFooter`, `FaqList`), `landing/`, `playground/` |
| `src/lib/playground/` | The playground application — see its guide |
| `src/content/` | Starlight content collection + this area's docs guide |
| `src/data/landing.ts`, `src/config/site.ts` | Landing copy; site identity (domain, apps, social, analytics, robots) |
| `src/styles/global.css` | 7-line wiring file: Tailwind layers + `@qql/ui` imports only |
| `packages/ui/` | `@qql/ui`: the single authored stylesheet, shared components, docs types |
| `scripts/` | The gates and generators (`check-*`, `verify-qql-docs`, `build-playground-wasm`, `generate-og`) |
| `public/` | Static assets and `_headers` (CSP rules, checked post-build) |
| `research/` | Design-direction and docs research notes (not shipped) |
| `DOCS_ARCHITECTURE.md` | Source-of-truth order and content IA |

## Non-negotiables

1. **One authored stylesheet.** All CSS lives in
   `packages/ui/src/styles/styles.css` (≤ 500 lines, enforced). One-off layout
   belongs in markup as Tailwind utilities. Never add a `.css` file or a
   `<style>` block.
2. **Generated files are generated.** `qql-keywords.generated.ts` comes from
   `cargo run -p qql-grammar-gen -- generate`; `packages/ui/src/styles/ec-theme.css`
   is vendored. Never hand-edit either.
3. **WASM always comes from this checkout.** `scripts/build-playground-wasm.mjs`
   builds `crates/qql-wasm`; the site resolves it through the
   `qql-wasm-current` Vite alias (runtime) and the tsconfig path (types). Do
   not depend on a published `qql-wasm` package — dev and deploy must not
   drift.
4. **Documentation QQL is executable or it does not exist.** Docs examples go
   in `{% qqlExample %}` and are parsed by `pnpm validate:qql`; raw ```qql
   fences fail the check.
5. **`@qql/ui` is the source of truth** for the shared UI package (a private
   consumer takes it from this repo, never the reverse). See
   `packages/ui/README.md`.
6. **Theme contract:** each page resolves
   `localStorage` `theme` → `starlight-theme` → `veristamp-theme` → system
   preference and sets `data-theme` on `<html>` before paint. Keep that order;
   keep `color-scheme` in the token layer.

## Working here

1. Read this file, then the area guide; read `DOCS_ARCHITECTURE.md` for
   anything content-related.
2. Make the smallest change that follows the existing conventions.
3. `pnpm check` — plus a browser pass for anything visual.
4. Commit with a scoped conventional message (`feat(playground): …`,
   `fix(website): …`, `docs(guides): …`).

CI (`deploy-site.yml`) runs `pnpm check` with the OG image cache restored, so
`generate-og` only re-renders changed pages.
