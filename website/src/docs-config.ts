import type { DocsFooterConfig, DocsHeaderConfig } from "@qql/ui-docs/types";
import { APPS, SITE } from "./config/site";

export const headerConfig: DocsHeaderConfig = {
	navItems: [
		{ name: "Docs", href: `${APPS.docs.url}/` },
		{ name: "Playground", href: `${APPS.playground.url}/` },
		{
			name: "GitHub",
			href: APPS.github.url,
			external: true,
		},
	],
};

export const footerConfig: DocsFooterConfig = {
	brand: {
		name: SITE.name,
		href: SITE.url,
		tagline:
			"Declarative vector search for Qdrant. SQL-like QQL across Rust, Python, Node, WASM, and edge.",
	},
	columns: [
		{
			title: "Product",
			items: [
				{
					label: "Getting Started",
					href: `${APPS.docs.url}/getting-started/`,
				},
				{ label: "Guides", href: `${APPS.docs.url}/guides/` },
				{ label: "Reference", href: `${APPS.docs.url}/reference/` },
				{ label: "SDKs", href: `${APPS.docs.url}/sdks/` },
				{ label: "Playground", href: `${APPS.playground.url}/` },
			],
		},
		{
			title: "Resources",
			items: [
				{ label: "CLI", href: `${APPS.docs.url}/tools/cli/` },
				{ label: "Examples", href: `${APPS.docs.url}/tools/examples/` },
				{
					label: "Releases",
					href: `${APPS.github.url}/releases`,
					external: true,
				},
				{ label: "Blog", href: APPS.blog.url, external: true },
			],
		},
		{
			title: "Connect",
			items: [
				{
					label: "GitHub",
					href: APPS.github.url,
					external: true,
				},
				{
					label: "Report an Issue",
					href: `${APPS.github.url}/issues`,
					external: true,
				},
			],
		},
	],
	bottom: {
		copyright: `© ${new Date().getFullYear()} ${SITE.org}. All rights reserved.`,
		note: "Built for developers. Runs anywhere.",
	},
};
