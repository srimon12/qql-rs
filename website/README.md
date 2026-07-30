# QQL Website (`qql.veristamp.in`)

Astro + Starlight site for QQL:

| Path | Content |
|------|---------|
| `/` | Landing |
| `/docs/` | qql-rs language, guides, SDKs, tools, and reference |
| `/playground/` | Nested Vite SPA (merged at deploy) |

## Packages

- `@qql/website` — this app
- `@qql/ui-docs` — vendored docs UI (from Veristamp `ui-docs`; local until shared package)

## Develop / build (all pnpm lives here)

```bash
cd website
pnpm install
pnpm dev          # http://localhost:4321
pnpm build        # → website/dist
pnpm validate:qql # build qql-wasm and parse every documented QQL example
pnpm check        # executable examples + production site build
```

No root `package.json` / pnpm workspace — only `website/`.

## Merge with playground

From **repo root** (after building playground with `VITE_BASE=/playground/` into `.playground-dist`):

```bash
node scripts/merge-qql-dist.mjs
# → dist-site/
```

## Deploy

GitHub Actions: `.github/workflows/deploy-site.yml`  
Pages project: `qql` · domain: `qql.veristamp.in`

## Documentation contract

The information architecture and source-of-truth rules live in
[`DOCS_ARCHITECTURE.md`](DOCS_ARCHITECTURE.md). Runnable QQL belongs in the
`qqlExample` Markdoc component so it is checked against a fresh Node-target
build of the current `qql-wasm` crate.

The playground build and merge remain a separate concern from documentation
content and validation.
