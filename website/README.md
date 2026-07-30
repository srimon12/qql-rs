# QQL Website (`qql.veristamp.in`)

Astro + Starlight site for QQL:

| Path | Content |
|------|---------|
| `/` | Landing |
| `/docs/` | Starlight documentation (copied content) |
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

## Note on docs content

Markdown under `src/content/` is a **copy** of the previous qql-docs tree.  
Refactor content later; theme/components/deploy land first.
