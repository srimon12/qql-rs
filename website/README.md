# QQL Website (`qql.veristamp.in`)

Astro + Starlight site for QQL:

| Path | Content |
|------|---------|
| `/` | Landing |
| `/docs/` | qql-rs language, guides, SDKs, tools, and reference |
| `/playground/` | Astro-native QQL editor, planner, fixture browser, and Qdrant runner |

## Packages

- `@qql/website` — this app
- `@qql/ui-docs` — vendored docs UI (from Veristamp `ui-docs`; local until shared package)

## Develop / build (all pnpm lives here)

```bash
cd website
pnpm install
pnpm dev          # build current web WASM, then serve http://localhost:4321
pnpm build        # build current web WASM → Astro site in website/dist
pnpm validate:qql # parse docs and every valid fixture with current Node WASM
pnpm check        # all executable QQL + web WASM + production Astro build
```

No root `package.json` / pnpm workspace — only `website/`.

## Playground

The playground is part of this Astro application. Its UI is split into reusable
Astro components under `src/components/playground`; one browser controller owns
CodeMirror, WASM lifetimes, policy rewrites, and Qdrant execution.

The preset browser is generated from every file in
`../language/v1/fixtures/valid`. `scripts/build-playground-wasm.mjs` builds
`../crates/qql-wasm` from the current checkout, so development and deployment
cannot silently drift to a published package.

## Deploy

GitHub Actions: `.github/workflows/deploy-site.yml`  
Pages project: `qql` · domain: `qql.veristamp.in`

## Documentation contract

The information architecture and source-of-truth rules live in
[`DOCS_ARCHITECTURE.md`](DOCS_ARCHITECTURE.md). Runnable QQL belongs in the
`qqlExample` Markdoc component so it is checked against a fresh Node-target
build of the current `qql-wasm` crate.

Every `qqlExample` also links its verified source into `/playground/?q=…`.
