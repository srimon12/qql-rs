#!/usr/bin/env node
// Build-time OG (open-graph) image generator.
//
// Replaces the Astro `src/pages/open-graph/[...slug].png.ts` prerender route:
// rendering ~56 SVGs through sharp inside `astro build` put every OG image on
// the critical build path with no cross-run caching. This script renders the
// exact same SVGs to `public/open-graph/*.png` (gitignored, served as static
// files at identical URLs) with two speedups:
//   1. parallel rendering across cores instead of Astro's prerender queue,
//   2. a content-hash manifest (`.cache/og-manifest.json`) that skips
//      unchanged slugs — in CI the outputs + manifest are restored via
//      actions/cache, so pushes that don't touch OG inputs regenerate ~0
//      images.
//
// The SVG template below is byte-identical to the retired route: same
// escaping, wrapping, layout, and lossless `sharp().png()` encoding.
// Bump TEMPLATE_VERSION when the design changes to force a full regeneration.
//
// Usage: node scripts/generate-og.mjs

import { createHash } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { cpus } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import { SITE } from "../src/config/site.ts";

const websiteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const docsRoot = join(websiteRoot, "src", "content", "docs");
const outDir = join(websiteRoot, "public", "open-graph");
const manifestPath = join(websiteRoot, ".cache", "og-manifest.json");

// Bump to invalidate every cached image when the template design changes.
const TEMPLATE_VERSION = 3;
const FALLBACK_DESCRIPTION = "Declarative vector search for Qdrant.";

function escapeXml(unsafe) {
	return unsafe
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&apos;");
}

function wrapText(text, maxCharsPerLine, maxLines) {
	const words = text.trim().split(/\s+/);
	const lines = [];
	let currentLine = "";

	for (const word of words) {
		if (!currentLine || `${currentLine} ${word}`.length <= maxCharsPerLine) {
			currentLine = `${currentLine} ${word}`.trim();
		} else {
			lines.push(currentLine);
			if (lines.length === maxLines) {
				lines[maxLines - 1] = `${lines[maxLines - 1].replace(/[.,;:]$/, "")}…`;
				return lines;
			}
			currentLine = word;
		}
	}
	if (currentLine) lines.push(currentLine);
	return lines;
}

function getCategoryFromSlug(slug) {
	if (slug === "home" || slug === "og-image")
		return "DECLARATIVE VECTOR SEARCH";
	if (slug === "playground") return "INTERACTIVE PLAYGROUND";
	if (slug.includes("getting-started")) return "GETTING STARTED";
	if (slug.includes("guides")) return "PRODUCTION GUIDE";
	if (slug.includes("language")) return "LANGUAGE REFERENCE";
	if (slug.includes("edge")) return "IN-PROCESS EDGE RUNTIME";
	if (slug.includes("sdks")) return "NATIVE SDKs";
	if (slug.includes("tools")) return "DEVELOPER TOOLING";
	if (slug.includes("reference")) return "API & ERROR SPECIFICATION";
	if (slug.includes("contributing")) return "CONTRIBUTING";
	return "DOCUMENTATION";
}

const SPECIMEN_CODE = [
	[
		["QUERY ", "ffab98"],
		["'chest pain' ", "9fe3b8"],
		["FROM ", "ffab98"],
		["medical", null],
	],
	[
		["USING ", "ffab98"],
		["dense", null],
	],
	[
		["WHERE ", "ffab98"],
		["department ", null],
		["= ", "8b8880"],
		["'cardio'", "9fe3b8"],
	],
	[
		["SHARD ", "ffab98"],
		["'hospital-east'", "9fe3b8"],
	],
	[
		["LIMIT ", "ffab98"],
		["5", "f2c983"],
		[";", "8b8880"],
	],
];

function specimenBody(description) {
	const codeLines = SPECIMEN_CODE.map(
		(tokens, i) =>
			`    <text x="32" y="${76 + i * 25}" xml:space="preserve" font-family="DejaVu Sans Mono, monospace" font-size="14.5" fill="#dedbd3">${tokens
				.map(([text, color]) =>
					color
						? `<tspan fill="#${color}">${escapeXml(text)}</tspan>`
						: escapeXml(text),
				)
				.join("")}</text>`,
	).join("\n");

	return `  <!-- Headline & description -->
  <text x="80" y="203" font-family="DejaVu Serif, Georgia, serif" font-size="58" fill="#f6f4ee" letter-spacing="-1">SQL for Qdrant.</text>
  <text x="80" y="246" font-family="DejaVu Sans, Arial, sans-serif" font-size="19" fill="#b6b3aa">${escapeXml(description)}</text>

  <!-- Specimen card -->
  <g transform="translate(80, 278)">
    <rect width="1040" height="224" rx="10" fill="#1c1b19" stroke="#2b2a26"/>
    <rect x="18" y="15" width="1" height="12" fill="#ba5442"/>
    <text x="32" y="25" font-family="DejaVu Sans Mono, monospace" font-size="12" fill="#b6b3aa">search.qql</text>
    <text x="1022" y="25" font-family="DejaVu Sans Mono, monospace" font-size="11" fill="#8b887e" text-anchor="end">QQL · MIT</text>
    <line x1="0" y1="36" x2="1040" y2="36" stroke="#2b2a26"/>
${codeLines}
    <line x1="0" y1="192" x2="1040" y2="192" stroke="#2b2a26"/>
    <text x="18" y="213" font-family="DejaVu Sans Mono, monospace" font-size="11" fill="#8b887e">200 OK</text>
    <text x="82" y="213" font-family="DejaVu Sans Mono, monospace" font-size="11" fill="#8b887e">POST /collections/medical/points/query</text>
    <text x="1022" y="213" font-family="DejaVu Sans Mono, monospace" font-size="11" fill="#8b887e" text-anchor="end">1.2ms</text>
  </g>`;
}

function editorialBody(title, description) {
	const titleLines = wrapText(title, 30, 2);
	const descLines = wrapText(description, 62, 3);
	const descStartY = 244 + (titleLines.length - 1) * 62 + 126;

	const titleTspans = titleLines
		.map(
			(line, i) =>
				`<tspan x="80" y="${244 + i * 62}">${escapeXml(line)}</tspan>`,
		)
		.join("\n");
	const descTspans = descLines
		.map(
			(line, i) =>
				`<tspan x="80" y="${descStartY + i * 30}">${escapeXml(line)}</tspan>`,
		)
		.join("\n");

	return `  <!-- Title & Description -->
  <g>
    <text font-family="DejaVu Serif, Georgia, serif" font-size="52" fill="#f6f4ee" letter-spacing="-0.8">
      ${titleTspans}
    </text>
    <text font-family="DejaVu Sans, Arial, sans-serif" font-size="19" fill="#b6b3aa">
      ${descTspans}
    </text>
  </g>`;
}

function generateSvg({ title, description, category, specimen }) {
	return `
<svg width="1200" height="630" viewBox="0 0 1200 630" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgGrad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#181816"/>
      <stop offset="55%" stop-color="#141413"/>
      <stop offset="100%" stop-color="#101010"/>
    </linearGradient>
    <pattern id="grid" width="36" height="36" patternUnits="userSpaceOnUse">
      <path d="M 36 0 L 0 0 0 36" fill="none" stroke="#262623" stroke-width="1" stroke-opacity="0.55"/>
    </pattern>
  </defs>

  <!-- Background -->
  <rect width="1200" height="630" fill="url(#bgGrad)"/>
  <rect width="1200" height="630" fill="url(#grid)" opacity="0.8"/>

  <!-- Outer frame border -->
  <rect x="36" y="36" width="1128" height="558" rx="16" fill="none" stroke="#2e2e2a" stroke-width="1.5"/>

  <!-- Top bar -->
  <g transform="translate(80, 54)">
    <!-- Veristamp mark: ring, double-slit V -->
    <mask id="vslit"><rect width="32" height="32" fill="#fff"/><line x1="23.54" y1="9.02" x2="16" y2="22.59" stroke="#000" stroke-width="1.6"/></mask>
    <circle cx="16" cy="16" r="12.64" fill="none" stroke="#b04930" stroke-width="2.56"/>
    <path fill="#f4efe6" mask="url(#vslit)" d="M6.698 9.998 L16 26.741 L25.302 9.998 L21.777 8.04 L16 18.439 L10.223 8.04 Z"/>
    <text x="46" y="25" font-family="DejaVu Serif, Georgia, serif" font-size="24" font-weight="400" fill="#f6f4ee" letter-spacing="-0.5">QQL</text>
    <text x="106" y="24" font-family="DejaVu Sans, Arial, sans-serif" font-size="13" fill="#8b887e" letter-spacing="0.4">/ ${escapeXml(SITE.org)}</text>
    <text x="1040" y="22" font-family="DejaVu Sans Mono, monospace" font-size="11" font-weight="700" fill="#d29084" letter-spacing="1.2" text-anchor="end">${escapeXml(category)}</text>
  </g>

${specimen ? specimenBody(description) : editorialBody(title, description)}

  <!-- Bottom status bar -->
  <g transform="translate(80, 535)">
    <line x1="0" y1="0" x2="1040" y2="0" stroke="#2a2a26" stroke-width="1"/>
    <text x="0" y="28" font-family="DejaVu Sans Mono, monospace" font-size="13" font-weight="600" fill="#f5f4ed" letter-spacing="0.5">qql.veristamp.in</text>
    <text x="175" y="28" font-family="DejaVu Sans, Arial, sans-serif" font-size="13" fill="#8b887e">SQL for Qdrant vector search</text>
    <text x="1040" y="28" font-family="DejaVu Sans Mono, monospace" font-size="12" fill="#d29084" text-anchor="end">Rust, Python, Node, WASM</text>
  </g>
</svg>`.trim();
}

const SPECIALS = [
	{
		slug: "home",
		title: "QQL: SQL for Qdrant Vector Search",
		description:
			"One query language for hybrid search, filters, mutations, and schema. Every runtime.",
	},
	{
		slug: "og-image",
		title: "QQL: SQL for Qdrant Vector Search",
		description:
			"One query language for hybrid search, filters, mutations, and schema. Every runtime.",
	},
	{
		slug: "playground",
		title: "QQL Playground: in-browser WASM parser and planner",
		description:
			"Interactive browser playground for QQL. Parse queries, inspect ASTs, verify planned execution routes, and test AST filter injection in real time.",
	},
];

/** Minimal frontmatter reader: docs only use single-line `title:`/`description:`. */
function readFrontmatter(path) {
	const source = readFileSync(path, "utf8");
	const match = source.match(/^---\r?\n([\s\S]*?)\r?\n---/);
	const data = {};
	if (!match) return data;
	for (const line of match[1].split("\n")) {
		const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
		if (!kv || kv[1] in data) continue;
		let value = kv[2].trim();
		if (
			(value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
			(value.startsWith("'") && value.endsWith("'") && value.length >= 2)
		) {
			value = value.slice(1, -1);
		}
		data[kv[1]] = value;
	}
	return data;
}

function filesUnder(directory) {
	return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
		const path = join(directory, entry.name);
		return entry.isDirectory() ? filesUnder(path) : [path];
	});
}

/**
 * Mirror Astro's content-collection id for docs: path relative to
 * `src/content/docs` minus extension, with a trailing `/index` collapsed to
 * the directory (so `docs/guides/index.mdoc` serves `/open-graph/docs/guides.png`,
 * matching DocsHead's `og:image` URL).
 */
function slugFromFile(path) {
	const rel = relative(docsRoot, path).split(sep).join("/");
	return rel.replace(/\.(mdoc|md)$/, "").replace(/\/index$/, "");
}

function collectEntries() {
	const entries = SPECIALS.map((special) => ({
		slug: special.slug,
		title: special.title,
		description: special.description,
		category: getCategoryFromSlug(special.slug),
		specimen: special.slug === "home" || special.slug === "og-image",
	}));
	for (const file of filesUnder(docsRoot).filter((path) =>
		/\.(mdoc|md)$/.test(path),
	)) {
		const frontmatter = readFrontmatter(file);
		const slug = slugFromFile(file);
		entries.push({
			slug,
			title: frontmatter.title || "Documentation",
			description: frontmatter.description || FALLBACK_DESCRIPTION,
			category: getCategoryFromSlug(slug),
			specimen: false,
		});
	}
	for (const entry of entries) {
		entry.hash = createHash("sha256")
			.update(
				JSON.stringify([
					TEMPLATE_VERSION,
					SITE.org,
					entry.title,
					entry.description,
					entry.category,
				]),
			)
			.digest("hex");
	}
	return entries;
}

function loadManifest() {
	try {
		const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
		if (manifest?.version === TEMPLATE_VERSION && manifest.files) {
			return manifest.files;
		}
	} catch {
		// Missing or stale manifest: regenerate everything.
	}
	return {};
}

async function mapPool(items, size, fn) {
	let next = 0;
	const workers = Array.from(
		{ length: Math.min(size, items.length) },
		async () => {
			while (next < items.length) {
				const item = items[next++];
				await fn(item);
			}
		},
	);
	await Promise.all(workers);
}

async function renderEntry(entry) {
	const svg = generateSvg(entry);
	const pngBuffer = await sharp(Buffer.from(svg)).png().toBuffer();
	const outPath = join(outDir, `${entry.slug}.png`);
	mkdirSync(dirname(outPath), { recursive: true });
	writeFileSync(outPath, pngBuffer);
}

/** Delete PNGs whose slug no longer exists (e.g. a doc page was removed). */
function pruneStale(slugs) {
	if (!existsSync(outDir)) return;
	for (const file of filesUnder(outDir).filter((path) =>
		path.endsWith(".png"),
	)) {
		const slug = relative(outDir, file)
			.split(sep)
			.join("/")
			.replace(/\.png$/, "");
		if (!slugs.has(slug)) {
			rmSync(file);
		}
	}
}

const entries = collectEntries();
const previous = loadManifest();
const slugs = new Set(entries.map((entry) => entry.slug));
const pending = entries.filter(
	(entry) =>
		previous[entry.slug] !== entry.hash ||
		!existsSync(join(outDir, `${entry.slug}.png`)),
);

const concurrency = Math.max(2, Math.min(8, cpus().length));
await mapPool(pending, concurrency, renderEntry);
pruneStale(slugs);

mkdirSync(dirname(manifestPath), { recursive: true });
writeFileSync(
	manifestPath,
	`${JSON.stringify(
		{
			version: TEMPLATE_VERSION,
			files: Object.fromEntries(
				entries.map((entry) => [entry.slug, entry.hash]),
			),
		},
		null,
		2,
	)}\n`,
);

console.log(
	`og: ${entries.length} images, ${pending.length} regenerated, ${
		entries.length - pending.length
	} reused from cache.`,
);
