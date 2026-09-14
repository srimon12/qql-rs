// @ts-check

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import markdoc from "@astrojs/markdoc";
import sitemap from "@astrojs/sitemap";
import starlight from "@astrojs/starlight";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";
import starlightLlmsTxt from "starlight-llms-txt";

const SITE_URL = "https://qql.veristamp.in";

// Brand syntax palette (design direction §4, --q-sx-*). Supplying these as
// expressive-code themes keeps docs code blocks and the landing comparator on
// the same colors instead of the stock Night Owl purple/blue.
const syntaxLight = {
	keyword: "#9c3d2b",
	operator: "#a8503c",
	string: "#166b48",
	number: "#7a5100",
	name: "#33322e",
	comment: "#6f6d64",
};
const syntaxDark = {
	keyword: "#ffab98",
	operator: "#d29084",
	string: "#9fe3b8",
	number: "#f2c983",
	name: "#dedbd3",
	comment: "#8b8880",
};

/**
 * Build one expressive-code theme from the brand syntax palette.
 * @param {string} name
 * @param {"dark" | "light"} type
 * @param {{ keyword: string; operator: string; string: string; number: string; name: string; comment: string }} c
 * @param {string} bg
 */
const qqlCodeTheme = (name, type, c, bg) => ({
	name,
	type,
	colors: { "editor.background": bg, "editor.foreground": c.name },
	tokenColors: [
		{
			scope: ["comment", "punctuation.definition.comment"],
			settings: { foreground: c.comment, fontStyle: "italic" },
		},
		{
			scope: ["string", "constant.character.escape"],
			settings: { foreground: c.string },
		},
		{
			scope: ["constant.numeric", "constant.language"],
			settings: { foreground: c.number },
		},
		{
			scope: ["keyword", "storage", "storage.type", "storage.modifier"],
			settings: { foreground: c.keyword },
		},
		{
			scope: ["keyword.operator", "operator"],
			settings: { foreground: c.operator },
		},
		{
			scope: [
				"punctuation",
				"punctuation.terminator",
				"punctuation.separator",
				"punctuation.definition",
				"meta.brace",
			],
			settings: { foreground: c.comment },
		},
		{
			scope: [
				"entity.name",
				"variable",
				"variable.other",
				"variable.parameter",
				"support.function",
				"support.class",
				"meta.function-call",
			],
			settings: { foreground: c.name },
		},
	],
});

// Tailwind v4 plugin returns Plugin[] which Vite's PluginOption type rejects under Astro 7.
const tailwind = /** @type {any} */ (tailwindcss());
const qqlGrammar = {
	...JSON.parse(
		readFileSync(
			new URL(
				"../editors/vscode/syntaxes/qql.tmLanguage.json",
				import.meta.url,
			),
			"utf8",
		),
	),
	name: "qql",
};

export default defineConfig(
	/** @type {import("astro").AstroUserConfig} */ ({
		site: SITE_URL,
		trailingSlash: "always",

		// Cloudflare Speed Brain refuses every `sec-purpose: prefetch` on this
		// zone's Pages projects ("disabled for worker requests" → 503), so
		// Astro's link prefetch only spams the console and never succeeds.
		// Read by Starlight's astro:config:setup hook (config.prefetch).
		prefetch: false,

		integrations: [
			sitemap(),
			starlight({
				expressiveCode: {
					themes: [
						qqlCodeTheme("qql-dark", "dark", syntaxDark, "#1c1b19"),
						qqlCodeTheme("qql-light", "light", syntaxLight, "#fbfaf5"),
					],
					// The site ships its own copy button (@qql/ui CopyButton +
					// scripts/copy.ts) on every code frame and docs page; EC's
					// built-in button is redundant.
					frames: { showCopyToClipboardButton: false },
					shiki: {
						// Reuse the VS Code TextMate grammar so documentation and editor
						// highlighting recognize the same QQL vocabulary.
						langs: [qqlGrammar],
					},
				},
				plugins: [starlightLlmsTxt({ projectName: "QQL" })],
				title: "QQL Documentation",
				favicon: "/favicon.ico",
				description:
					"Declarative vector search for Qdrant: QQL language, SDKs, CLI, and security patterns.",
				social: [
					{
						icon: "github",
						label: "GitHub",
						href: "https://github.com/srimon12/qql-rs",
					},
				],
				customCss: [
					"@fontsource-variable/geist/index.css",
					"@fontsource-variable/geist-mono/index.css",
					"@fontsource/newsreader/latin-300.css",
					"@fontsource/newsreader/latin-400.css",
					"./src/styles/global.css",
				],
				components: {
					Head: "./src/components/DocsHead.astro",
					Footer: "./src/components/DocsFooter.astro",
					Header: "./src/components/DocsHeader.astro",
				},
				sidebar: [
					{
						label: "Start",
						items: [
							{ label: "Overview", link: "/docs/" },
							{ label: "What is QQL?", link: "/docs/getting-started/" },
							{
								label: "Installation",
								link: "/docs/getting-started/installation/",
							},
							{
								label: "Quickstart",
								link: "/docs/getting-started/quickstart/",
							},
							{
								label: "Execution model",
								link: "/docs/getting-started/execution-model/",
							},
							{ label: "FAQ", link: "/docs/faq/" },
						],
					},
					{
						label: "Language",
						items: [{ autogenerate: { directory: "docs/language" } }],
					},
					{
						label: "Guides",
						items: [{ autogenerate: { directory: "docs/guides" } }],
					},
					{
						label: "Operations",
						items: [{ autogenerate: { directory: "docs/operations" } }],
					},
					{
						label: "Edge",
						items: [{ autogenerate: { directory: "docs/edge" } }],
					},
					{
						label: "SDKs",
						items: [{ autogenerate: { directory: "docs/sdks" } }],
					},
					{
						label: "Tools",
						items: [{ autogenerate: { directory: "docs/tools" } }],
					},
					{
						label: "Reference",
						items: [{ autogenerate: { directory: "docs/reference" } }],
					},
					{
						label: "Contributing",
						items: [{ autogenerate: { directory: "docs/contributing" } }],
					},
				],
			}),
			markdoc(),
		],
		vite: {
			plugins: [tailwind],
			optimizeDeps: {
				// The browser embedder imports this only on demand. Keep it out of the
				// playground's eager optimization path.
				exclude: ["@huggingface/transformers"],
			},
			resolve: {
				alias: {
					"qql-wasm-current": fileURLToPath(
						new URL("./.wasm/qql-wasm/qql_wasm.js", import.meta.url),
					),
				},
			},
		},
	}),
);
