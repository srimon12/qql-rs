# @qql/ui

The QQL site's UI package. It owns:

- **`src/styles/styles.css`** — the single authored stylesheet for the landing page, the playground, and the Starlight docs (tokens, primitives, third-party overrides). Loaded by `website/src/styles/global.css`; nothing else is allowed to add CSS.
- **`src/styles/ec-theme.css`** — the vendored expressive-code theme, excluded from the budget below. Do not edit by hand; re-vendor when the expressive-code major changes.
- **Astro components** — `BrandMark`, `ThemeToggle`, `DocsHeader`, `DocsFooter`, `QqlExample`, `Terminal`, `TechBadge`, `GlassCard`, `ApiField`. These render docs and site chrome; the site wires them through Starlight component overrides and `markdoc.config.mjs`.
- **`src/types.ts`** — the config shapes for the docs header and footer.

## The 500-line budget

`website/scripts/check-css-budget.mjs` (`pnpm check:css`, first step of `pnpm check`) fails when:

1. `src/styles/styles.css` exceeds **500 lines**;
2. any other non-excluded `.css` file appears under `website/src` or `website/packages/*`;
3. `website/src/styles/global.css` (the wiring file) exceeds 15 lines.

The budget exists because the previous four-file, ~2,700-line sprawl drifted for two months. Two rules keep it honest:

- **New styles go into `styles.css` or nowhere.** One-off layout, spacing, and responsive arrangement belong in markup as Tailwind utilities, not in a new file or a `<style>` block.
- **Delete, don't port.** When a rule stops being used, remove it; never comment it out.

## Consumption from another repo

Veristamp (private) consumes this package **from the public qql-rs repo**, never the other way around: a pnpm git dependency pinned to a commit (`"@qql/ui": "github:srimon12/qql-rs#<sha>&path:/website/packages/ui"`), or a vendor script that copies this directory and records the SHA. Publish to npm only if an outside consumer appears.
