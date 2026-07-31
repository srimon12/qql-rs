**Audited:** \`apps/qql-docs\` (Veristamp monorepo)  
**Target comparison:** \`website/\` (qql-rs monorepo)  
**Date:** 2026-07-31  

---

## 1. Tech Stack / Build / App Structure

### Legacy (Veristamp \`apps/qql-docs\`)

- **Framework:** Astro 7.0.6 + Starlight 0.41.3 documentation theme
- **Content pipeline:** Markdoc (\`.mdoc\` files) via \`@astrojs/markdoc\` + \`@astrojs/starlight-markdoc\`
- **CSS:** Tailwind CSS v4 via \`@tailwindcss/vite\` (Vite 8 / Rolldown compatible)
- **Bundler:** Vite 8 / Rolldown (Astro 7 default)
- **Search:** Pagefind (bundled by Starlight at build time)
- **Fonts:** IBM Plex Sans (UI), Instrument Serif (headings), JetBrains Mono (code) — all served via \`@fontsource/*\` npm packages
- **Hosting:** Static export (\`astro build\` → \`dist/\`), deployed behind \`_headers\` with CSP
- **Monorepo:** pnpm workspace, shares \`@veristamp/config\` and \`@veristamp/ui-docs\` packages
- **Domain:** \`qql-go.veristamp.in\` (product docs at \`/docs/...\`)

Key config files:  
\`/data/codebases/Veristamp/apps/qql-docs/astro.config.mjs\`  
\`/data/codebases/Veristamp/apps/qql-docs/package.json\`  
\`/data/codebases/Veristamp/apps/qql-docs/markdoc.config.mjs\`  
\`/data/codebases/Veristamp/apps/qql-docs/src/content.config.ts\`

### Architecture

\`\`\`
apps/qql-docs/
├── astro.config.mjs          # Starlight + Markdoc + Tailwind v4 + analytics
├── markdoc.config.mjs        # Custom %% tags (terminal, glassCard, apiField, etc.)
├── src/
│   ├── content.config.ts     # Astro content collection (docs loader + schema)
│   ├── docs-config.ts        # Nav/footer link configuration objects
│   ├── styles/global.css     # Imports starlight-tailwind → tailwind → ui-docs/global.css
│   ├── pages/
│   │   ├── index.astro       # Standalone marketing landing page (no Starlight)
│   │   └── robots.txt.ts     # Dynamic robots.txt from shared config
│   ├── components/
│   │   ├── DocsFooter.astro  # Thin wrapper → @veristamp/ui-docs/DocsFooter
│   │   └── DocsHeader.astro  # Thin wrapper → @veristamp/ui-docs/DocsHeader
│   └── content/docs/
│       ├── 404.md            # Starlight splash 404
│       ├── docs/             # All documentation under /docs/...
│       │   ├── index.mdoc    # JS redirect to /docs/getting-started/
│       │   ├── getting-started/ (4 pages)
│       │   ├── guides/ (9 pages)
│       │   ├── reference/ (5 pages)
│       │   ├── sdks/ (4 pages: go, python, typescript)
│       │   ├── gateway/ (7 pages)
│       │   ├── examples/ (1 page)
│       │   ├── contributing/ (2 pages)
│       │   ├── changelog/ (11 release note pages)
│       │   └── landing/ (4 thin marketing fragment pages, noindex)
│   └── public/
│       ├── _headers          # CSP, HSTS, cache headers
│       ├── favicon.ico
│       └── og-image.png
└── dist/                     # Static build output
\`\`\`

---

## 2. Content / Route System

### Content Storage

All documentation lives as \`.mdoc\` (Markdoc) files in \`src/content/docs/docs/\`. The \`docsLoader\` from Starlight maps this directory tree to the \`/docs/...\` URL structure automatically. Frontmatter uses Starlight conventions (\`title\`, \`description\`, \`sidebar.order\`, \`pagefind\`, \`template\`).

### Authored Content Count

| Section | Page Count | Notes |
|---------|-----------|-------|
| Getting Started | 4 | installation, configuration, quickstart, index |
| Guides | 9 | hybrid-search, batch-operations, pagination, filtering, recommendations, grouped-search, multivector-colbert, agent-mode, multi-stage-retrieval, scripting, score-boosting, convert-rest-to-qql |
| Reference | 5 | syntax, filters, cli, compatibility, boost-expressions |
| SDKs | 4 | go, python (×2 incl. migration), typescript |
| Gateway | 7 | index, quickstart, auth, rate-limiting, templates, policy-engine, ast-injection, audit-logging, proto-reference, rpc-reference |
| Examples | 1 | Single page with 6 workflow references to GitHub |
| Contributing | 2 | development, releases |
| Changelog | 11 | Release notes 0.1.0 through 0.5.0 |
| Landing (thin) | 4 | hero, features, performance, architecture (noindex) |
| **Total** | **~47** | |

### Route Layout

- \`/\` — Custom standalone Astro page (no Starlight shell), built from \`src/pages/index.astro\`
- \`/docs/...\` — All Starlight-wrapped documentation pages
- \`/404/\` — Starlight splash 404
- \`/robots.txt\` — Dynamic generation
- \`/llms-full.txt\`, \`/llms-small.txt\` — LLM-friendly docs export (Starlight plugin)

### Sitemap & SEO

- Sitemap at \`/sitemap-index.xml\` (excludes \`/docs/landing/*\`)
- Full Open Graph + Twitter card meta on the landing page
- JSON-LD structured data (SoftwareApplication + FAQPage) on the landing page
- \`robots.txt\` blocks AI crawlers via \`generateRobotsTxt()\` from shared config
- Pagefind search indexes all content except landing fragment pages

### Landing Page Pattern (index.astro)

The landing page is a **completely separate hand-built page** — no Starlight shell, no shared layout. It:
- Sets \`<html data-theme="dark">\` explicitly (no theme toggle)
- Uses hardcoded color values (\`bg-[#141413]\`, \`text-[#e6e4e0]\`)
- Inlines its own header and footer HTML
- Imports \`TechBadge\`, \`Terminal\`, \`GlassCard\` from \`@veristamp/ui-docs\`
- Has a terminal simulation with example shell commands (non-interactive, static)

**This creates a maintenance burden** — header, footer, analytics, and theme are duplicated between the landing page and the Starlight docs shell.

---

## 3. Visual Design Tokens — Centralized vs Scattered

### Centralized Theme (\`@veristamp/ui-docs/src/styles/global.css\`)

The single CSS file at \`packages/ui-docs/src/styles/global.css\` is the **design authority**. It:

1. **Overrides Starlight CSS custom properties** for the dark theme:
   \`\`\`css
   :root[data-theme="dark"] {
     --sl-color-bg: #141413;
     --sl-color-bg-nav: rgba(20, 20, 19, 0.7);
     --sl-color-text: #e6e4e0;
     --sl-color-accent: #ba5442;         /* Terracotta / rust accent */
     --sl-color-accent-high: #ffcfbf;
     --sl-color-gray-1 through --sl-color-gray-6;
   }
   \`\`\`

2. **Light mode** inverts: \`--sl-color-bg: #f5f4ed\`, same accent

3. **Typography cascade**: All headings forced to Instrument Serif (weight 300), specific font sizes at h1 (3.5rem), h2 (2.25rem), h3 (1.75rem)

4. **Inline code style**: bordered + accent colored

5. **Glassmorphism** on cards/asides: \`backdrop-filter: blur(12px)\`, gradient backgrounds, subtle borders

6. **Sidebar styling**: reduced opacity, hover background, accent highlight on active page

7. **Custom scrollbar**: 8px, using gray-5 thumb

8. **Starlight component overrides**: Cards, tabs, steps, asides — all reshaped with glassmorphism

The app's own \`src/styles/global.css\` is a 7-line layer importer:
\`\`\`css
@layer base, starlight, theme, components, utilities;
@import "@astrojs/starlight-tailwind";
@import "tailwindcss/theme.css" layer(theme);
@import "tailwindcss/utilities.css" layer(utilities);
@import "@veristamp/ui-docs/global.css";
\`\`\`

### Scattered / Inline Styling

Despite the centralized theme, **significant scattered styling exists**:

1. **Landing page** (\`src/pages/index.astro\`): ~300 lines of inline Tailwind classes, hardcoded colors (e.g., \`bg-[#141413]\`, \`text-[#e6e4e0]\`). The \`data-theme="dark"\` is hardcoded on the html element. This duplicates what the centralized theme provides.

2. **\`@veristamp/ui-docs\` components**: Each component scopes its own styles via \`<style>\` blocks:
   - \`Terminal.astro\`: window-button colors hardcoded (\`#ff5f56\`, \`#ffbd2e\`, \`#27c93f\`)
   - \`TechBadge.astro\`: variant colors hardcoded (\`#c084fc\`, \`#22d3ee\`, \`#fb923c\`, \`#a3e635\`)
   - \`ApiField.astro\`: required badge uses hardcoded \`#ef4444\`
   - \`DocsFooter.astro\`: gradient line, responsive grid layout (scoped)
   - \`DocsHeader.astro\`: nav-link pill styles, search input border-radius, gradient site title
   - \`PackageManager.astro\`: client-side JS for tab switching

### Design Token Summary

| Token | Where Defined | Consistency |
|-------|--------------|-------------|
| Color palette (bg, text, accent, grays) | \`ui-docs/global.css\` as \`--sl-color-*\` vars | ✅ Centralized in CSS custom properties |
| Font stack | \`ui-docs/global.css\` | ✅ Centralized |
| Heading styles | \`ui-docs/global.css\` element selectors | ✅ Centralized |
| Code styles | \`ui-docs/global.css\` | ✅ Centralized |
| Card / Aside / Tab glassmorphism | \`ui-docs/global.css\` | ✅ Centralized |
| Component-specific colors | Component \`<style>\` blocks | ⚠️ Hardcoded per component |
| Landing page colors | \`index.astro\` inline classes | ❌ Duplicated, hardcoded |
| Responsive breakpoints | Tailwind defaults + component media queries | ⚠️ Inconsistent (some at 38rem, 40rem, 50rem) |

**Verdict:** The design token system is well-centralized for the Starlight docs shell. The landing page is the primary offender with duplicated styling. Component-level hardcoded colors are acceptable for small decorative values but should be driven from CSS variables for a theming system.

---

## 4. Reusable Layout and Documentation Components

### Shared Package: \`@veristamp/ui-docs\`

Exported components (all Astro):

| Component | File | Purpose | Usage |
|-----------|------|---------|-------|
| \`DocsFooter\` | \`src/components/DocsFooter.astro\` (213 lines) | Brand footer with 3-column nav, gradient divider, edit/pagination | Starlight Footer override |
| \`DocsHeader\` | \`src/components/DocsHeader.astro\` (168 lines) | Nav bar with site title, Docs/GitHub links, search, theme toggle, language select | Starlight Header override |
| \`ThemeToggle\` | \`src/components/ThemeToggle.astro\` (107 lines) | Custom <custom-theme-toggle> web component, sun/moon Lucide icons, syncs to localStorage | Internal to DocsHeader |
| \`Terminal\` | \`src/components/Terminal.astro\` (101 lines) | macOS-style terminal chrome with title bar, dots, renders Code inside | Markdoc \`{% terminal %}\` tag |
| \`GlassCard\` | \`src/components/GlassCard.astro\` (36 lines) | Glassmorphism card with optional title, flexible slot | Markdoc \`{% glassCard %}\` tag |
| \`TechBadge\` | \`src/components/TechBadge.astro\` (77 lines) | Colored pill badge with dot, hover glow, 6 variants | Markdoc \`{% techBadge %}\` tag |
| \`ApiField\` | \`src/components/ApiField.astro\` (101 lines) | API parameter field: name, type, required/optional badge | Markdoc \`{% apiField %}\` tag |
| \`PackageManager\` | \`src/components/PackageManager.astro\` (114 lines) | Tabbed pnpm/npm/yarn/bun command display with client JS | Markdoc \`{% packageManager %}\` tag |
| \`Kbd\` | \`src/components/Kbd.astro\` (30 lines) | Keyboard shortcut visual | Markdoc \`{% kbd %}\` tag |

### Markdoc Custom Tags (\`markdoc.config.mjs\`)

All custom tags map one-to-one to the components above, plus a \`grid\` div tag for layout.

### App-level Wrappers

\`src/components/DocsFooter.astro\` and \`DocsHeader.astro\` are 6-line thin wrappers that pass config from \`docs-config.ts\` to the ui-docs components. This is a clean pattern — the config is separated from rendering.

### Landing Page Components (Not Reusable)

The landing page (\`src/pages/index.astro\`) contains its own header and footer HTML inline. These could have used the same ui-docs components but don't. This is a missed reuse opportunity.

---

## 5. Code Examples and Interactive / Try-It Patterns

### Current State: Static Only

**Every code example in the legacy docs is static text.** There is zero interactivity:

1. **\`{% terminal %}\` blocks**: macOS-terminal-styled <pre> blocks with bash/shell commands. Rendered with expressive-code for syntax highlighting. **Not runnable — copy-only.**

2. **Standard fenced code blocks** (\`\`\`sql, \`\`\`go, \`\`\`bash\`\`\`): Rendered by Starlight's expressive-code with copy button (via \`starlight-copy-button\` plugin). **Not runnable.**

3. **Examples page** (\`/docs/examples/\`): Links to GitHub \`examples/\` directory. User must clone the repo and run scripts manually. No in-browser execution.

4. **Landing page terminal**: Simulated shell session. Purely decorative.

5. **Copy button**: Available on all code blocks via \`starlight-copy-button\`.

6. **LLM export**: Full docs available as \`/llms-full.txt\` and \`/llms-small.txt\` for AI training/context.

### What's Missing vs Target

There is **no \`qqlExample\` component**, no playground integration, no WASM-based query execution, no \`?q=\` URL sharing, and no fixture-driven preset system.

### \`PackageManager\` Client Interactivity

The only interactive client component is \`PackageManager.astro\` — it has vanilla JS tab switching. This is minimal, self-contained, and works without a framework.

---

## 6. Accessibility / Responsiveness

### Strengths

1. **Semantic HTML**: Astro/Starlight outputs proper landmarks (\`<header>\`, \`<nav>\`, \`<main>\`, \`<footer>\`)
2. **Skip link**: \`<a class="sl-skip-link"\` to jump to content
3. **ARIA attributes**: \`aria-current="page"\` on active nav, \`aria-label\` on theme toggle, \`aria-hidden\` on decorative icons
4. **Focus styles**: \`:focus-visible\` outline on nav links, search input focus glow
5. **Dark/light mode**: Full theme support with localStorage persistence
6. **Responsive grid**: \`grid-cols-1 md:grid-cols-2\` patterns, responsive footer layout (1 col → 4 cols)
7. **Font scaling**: Uses \`rem\` and \`var(--sl-text-*)\` variables, respects browser zoom
8. **Code blocks**: Proper \`<pre>\` + \`<code>\` semantics, copy button
9. **CSP headers**: Comprehensive Content-Security-Policy in \`_headers\`
10. **noscript redirect**: \`/docs/index.mdoc\` has a \`<noscript>\` fallback for the JS redirect

### Defects / Gaps

1. **No keyboard-triggered theme toggle indicator**: The \`ThemeToggle\` button lacks a \`role\` and keyboard event for toggling theme via Enter/Space (though it's a <button> so it gets native keyboard handling — actually fine on second look).
2. **Missing \`lang\` on landing page**: The \`<html>\` has \`lang="en"\` — OK.
3. **Color contrast**: The gray text colors (\`--sl-color-gray-2: #b8b6b0\`, \`--sl-color-gray-3: #8a887d\`) on \`--sl-color-bg: #141413\` may be below WCAG AA for small text. This should be checked with a contrast tool.
4. **No reduced-motion support**: Transitions (\`transition-all\`, \`duration-200\`, hover animations) don't respect \`prefers-reduced-motion\`
5. **Code block font size**: Very small (\`0.72rem\` in qql-example caption) could be hard to read
6. **Terminal title area**: Uses flexbox centering but the title could overflow on narrow screens
7. **No print styles**: Other than what Starlight provides; the built output shows \`print.css\` but custom components aren't audited for print
8. **TechBadge hover tooltip**: Missing title/aria-label for expanded meaning of abbreviated names
9. **Search keyboard shortcut**: \`Ctrl+K\` shown but only on desktop (\`sl-hidden md:sl-block\`) — mobile users can't discover it

### Responsiveness Assessment

- **Starlight sidebar**: Collapses on small screens with hamburger menu (Starlight built-in)
- **Landing page**: Responsive with \`px-6\`, \`max-w-5xl\`, grid breakpoints
- **Footer grid**: Single column → 4 columns at 40rem
- **Header**: Grid layout at 50rem, stack otherwise
- **Terminal component**: Full width, content truncation possible on very small screens
- **Overall**: Functional but not optimized for mobile beyond what Starlight provides

---

## 7. Assets / Dependencies

### Direct Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| \`astro\` | ^7.0.6 | Framework |
| \`@astrojs/starlight\` | ^0.41.3 | Documentation theme |
| \`@astrojs/markdoc\` | ^2.0.3 | Markdoc content pipeline |
| \`@astrojs/starlight-markdoc\` | ^0.7.0 | Starlight × Markdoc integration |
| \`@astrojs/sitemap\` | ^3.7.3 | Sitemap generation |
| \`@astrojs/starlight-tailwind\` | ^5.0.0 | Starlight + Tailwind bridge |
| \`@tailwindcss/vite\` | ^4.3.2 | Tailwind v4 Vite plugin |
| \`tailwindcss\` | ^4.3.2 | Utility CSS |
| \`@fontsource/ibm-plex-sans\` | ^5.2.8 | UI font |
| \`@fontsource/instrument-serif\` | ^5.2.8 | Heading font |
| \`@fontsource/jetbrains-mono\` | ^5.2.8 | Code font |
| \`sharp\` | ^0.35.3 | Image processing |
| \`starlight-copy-button\` | GitHub ref | Copy-to-clipboard on code blocks |
| \`starlight-llms-txt\` | ^0.11.0 | LLM-friendly docs export |
| typescript | ^7.0.1-rc | Type checking |

### Internal Workspace Dependencies

| Package | Content |
|---------|---------|
| \`@veristamp/config\` | Site config (URLs, app names, analytics IDs, robots.ts) |
| \`@veristamp/ui-docs\` | All documentation UI components + theme CSS |

### Static Assets

| File | Role |
|------|------|
| \`public/favicon.ico\` | Favicon |
| \`public/og-image.png\` | Open Graph share image |
| \`public/_headers\` | Cloudflare/static-host headers with CSP |

### Build Output

\`dist/\` contains:
- Static HTML per route
- \`_astro/\` hashed assets (CSS, JS)
- \`pagefind/\` search index (WASM + fragment files)
- \`llms-full.txt\`, \`llms-small.txt\`
- \`sitemap-index.xml\`
- \`robots.txt\`
- \`404.html\`

---

## 8. Reusable Ideas vs Legacy Elements Not Worth Carrying Over

### ✅ Reusable Ideas (Keep / Adapt for qql-rs)

1. **Centralized design system via workspace package** (\`@veristamp/ui-docs\`): The pattern of a shared UI package with exported components and a global CSS theme is solid. The qql-rs website already inherits this pattern with its own \`packages/ui-docs\`.

2. **Starlight + Markdoc + Tailwind v4 stack**: Well-proven. The qql-rs website already uses the same stack (identical \`astro.config.mjs\` structure).

3. **Custom Markdoc tags for docs components** (\`terminal\`, \`glassCard\`, \`apiField\`, \`techBadge\`, \`kbd\`, \`packageManager\`): These abstractions keep content clean. The qql-rs website already has its own \`markdoc.config.mjs\` extending these.

4. **LLM export via \`starlight-llms-txt\`**: Zero-effort AI-readable docs export. Already present in qql-rs.

5. **Copy button on code blocks** (\`starlight-copy-button\`): Essential UX. Already present in qql-rs.

6. **Site title gradient effect**: \`background: linear-gradient(135deg, var(--sl-color-white) 40%, var(--sl-color-accent-high) 100%)\` with \`-webkit-background-clip: text\` — visually striking and easy to port.

7. **Glassmorphism card system**: The \`.premium-card\` pattern with backdrop-filter, gradient backgrounds, and hover lift is reusable.

8. **Responsive footer grid**: The 1fr → 1.5fr 1fr 1fr 1fr grid at 40rem is well-designed.

9. **Security-focused \`_headers\`**: CSP, HSTS, X-Frame-Options, Permissions-Policy are well-tuned (including wasm-unsafe-eval for Pagefind).

10. **Thin wrappers for component overrides**: \`DocsFooter.astro\` and \`DocsHeader.astro\` as thin pass-throughs to shared components is a clean separation.

### ❌ Legacy Elements / Anti-Patterns (Do Not Carry Over)

1. **Standalone landing page duplication** (\`src/pages/index.astro\`): The landing page reinvents header, footer, theme, and layout. The qql-rs website's approach of a SiteHeader/SiteFooter that wraps both the landing and docs shell is superior.

2. **No playground / no interactivity**: The biggest gap. All code examples are read-only. The new website must maintain its \`qqlExample\` → playground integration.

3. **Go-focused content**: The legacy docs cover \`qql-go\` CLI, Go SDK, and Connect gateway — all retired. The new website correctly focuses on \`qql-rs\`.

4. **Changelog as documentation pages**: 11 release note files in \`content/docs/docs/changelog/releases/\`. The new website delegates changelog to GitHub Releases (per DOCS_ARCHITECTURE.md).

5. **Marketing landing fragments under /docs/**: The \`/docs/landing/hero\`, \`features\`, \`performance\`, \`architecture\` pages are hidden from search and sidebar but pollute the content tree. The new site properly keeps marketing on the standalone landing page.

6. **Client JS bare for PackageManager tabs**: While functional, it's vanilla JS without proper cleanup. Could use a web component pattern (like ThemeToggle does with \`customElements.define\`).

7. **Hardcoded variant colors in TechBadge**: Six variant colors are hardcoded in the component. A more flexible token-based approach would be better.

8. **Landing page dark-only**: The landing page hardcodes \`data-theme="dark"\` and doesn't respect user preference. The docs shell has proper theme switching.

9. **Missing \`prefers-reduced-motion\`**: All interactive animations lack motion-sensitivity checks.

10. **Thin marketing pages (\`/docs/landing/\*\`)**: These are excluded from search, sidebar, and sitemap but still get built. Better to not have them at all in the content collection.

---

## 9. Conceptual Comparison to Target (qql-rs Website)

### Architecture Comparison

| Dimension | Legacy (\`apps/qql-docs\`) | Target (\`website/\` in qql-rs) |
|-----------|---------------------------|--------------------------------|
| **Framework** | Astro 7 + Starlight 0.41 + Markdoc | Same stack |
| **Theme system** | \`@veristamp/ui-docs/global.css\` | \`website/packages/ui-docs/global.css\` (forked) |
| **Content format** | \`.mdoc\` files | Same |
| **Landing page** | Standalone, inline styles | Standalone, but shares SiteHeader/SiteFooter |
| **Docs components** | terminal, glassCard, apiField, techBadge, kbd, packageManager | Same, plus **\`qqlExample\`** |
| **Code interactivity** | None (static copy-only) | **\`{% qqlExample %}\`** → \`/playground/?q=...\` links |
| **Playground** | Does not exist | **\`/playground/\`** route with CodeMirror, WASM parsing, live execution |
| **Playground presets** | None | **Auto-generated from \`language/v1/fixtures/\`** at build time |
| **WASM SDK** | None | **\`crates/qql-wasm\`** compiled to browser package |
| **Content source of truth** | Hand-written docs | **Canonical: \`language/v1/fixtures/\`** → prose derived from grammar |
| **Changelog** | 11 Markdoc files | Delegated to GitHub Releases |
| **SDKs documented** | Go (retired), Python, TypeScript | Rust, Python, Node.js, WASM |
| **Gateway docs** | Full section (7 pages) | Removed (product no longer includes gateway) |
| **Search** | Pagefind | Same |
| **LLM export** | starlight-llms-txt plugin | Same |
| **Analytics** | Umami (\`stats.veristamp.in\`) | Same or similar |

### Interactive Playground Deep Dive

The target playground (\`/data/codebases/qql-rs/website/src/pages/playground/index.astro\`) is a **major architectural addition**:

1. **CodeMirror editor** with QQL syntax highlighting, completion, and linting
2. **WASM-based parsing** via \`qql-wasm\` — parses, analyzes, and compiles QQL in the browser
3. **Inspector panel** showing: parse tree (AST), compiled routes (wire), tokens, execution plan, response, metrics
4. **Live execution** against a configurable Qdrant endpoint
5. **Policy injection simulation** (tenant isolation filter)
6. **Browser embedder** for local embedding generation
7. **Export** to Python, Node.js, Rust, and curl
8. **Presets** auto-generated from all valid v1 fixtures
9. **\`?q=\` URL parameter** for sharing playground queries from docs

The \`QqlExample.astro\` component (\`website/packages/ui-docs/src/components/QqlExample.astro\`) bridges docs and playground:
- Renders code with a "WASM verified" badge
- Has a "Try in playground ↗" link with \`/playground/?q=\${encodeURIComponent(code)}\`
- Uses the same \`.premium-card\` glassmorphism pattern as the legacy Terminal component

### Summary of Gaps in Legacy

| Gap | Severity | Impact |
|-----|----------|--------|
| No interactive code execution | **Critical** | Users can't experiment with QQL queries |
| No WASM integration | **Critical** | Missing browser-side parsing/validation |
| Duplicated landing page | **Medium** | Maintenance burden, theme drift |
| No fixture-drive content | **Medium** | Docs can drift from grammar |
| Go/gateway content obsolete | **Medium** | Wrong product documented |
| No playground | **High** | Key feature for developer adoption |
| Hardcoded colors on landing | **Low** | Cosmetic |

---

## 10. Migration Recommendation

### Strategy: Selective Harvest, Not Port

Do **not** attempt to port the legacy app wholesale to qql-rs. The target is already built on the same stack and is architecturally superior in every dimension. Instead:

### Phase 1 — Harvest Reusable Content (1-2 days)

1. **Review each legacy \`.mdoc\` page** for prose that describes QQL concepts still valid in v1 (syntax, filtering, hybrid search patterns). These are primarily the "Guides" and "Reference" sections.
2. **Discard**: All gateway pages (7), Go SDK pages (2), changelog (11 files), landing fragments (4 files).
3. **Adapt**: Hybrid search, filtering, batch operations, pagination, CTE patterns, BOOST expressions — the QQL language surface is largely the same, only the execution backend changed. These prose sections can be reworked into the new IA sections (Language → Guides → Reference).
4. **Rewrite**: Getting started, installation, and quickstart — the CLI changed from \`qql-go\` to the Rust binary.

### Phase 2 — Harvest UI Components (0.5 days)

1. The \`@veristamp/ui-docs\` components (Terminal, GlassCard, TechBadge, ApiField, Kbd) are already forked into \`website/packages/ui-docs\`. Compare the forks and cherry-pick any polish improvements from the legacy side.
2. The CSS theme (\`global.css\`) is already forked. The qql-rs version may need light adjustments but the base visual identity (dark terracotta-accent, Instrument Serif headings, glassmorphism) is already ported.

### Phase 3 — Verify Playground Integration (1 day)

1. Ensure every existing doc page that contains a QQL code block has been converted to a \`{% qqlExample %}\` tag.
2. Verify that \`language/v1/fixtures/\` covers the syntax patterns described in migrated prose.
3. Run the docs verification script (\`scripts/verify-qql-docs.mjs\`) against all migrated pages.

### Phase 4 — Clean Up (0.5 days)

1. Archive the \`apps/qql-docs\` directory in the Veristamp monorepo.
2. Remove \`qqlGo\` entry from \`@veristamp/config\`.
3. Confirm no cross-references remain from legacy URLs.

### What NOT to Migrate

- 🚫 Gateway documentation (product removed)
- 🚫 Go SDK documentation (retired)
- 🚫 Changelog as content pages (use GitHub Releases)
- 🚫 Landing fragment pages under /docs/
- 🚫 Standalone landing page with duplicated header/footer
- 🚫 packageManager component (not relevant — qql-rs has no npm package yet)
- 🚫 Any inline CSS or hardcoded colors that duplicate the theme system
`;
