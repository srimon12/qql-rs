#!/usr/bin/env node
// Post-build link checker for the docs site.
//
// Verifies that every same-site `/docs/...` link resolves to a generated HTML
// page in `dist/` and that every `#fragment` points at an existing `id` on the
// target page. Runs after `astro build` via `pnpm check`.
//
// Usage: node scripts/check-site-anchors.mjs [distDir]

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const websiteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const distRoot = resolve(process.argv[2] ?? join(websiteRoot, "dist"));

function filesUnder(directory) {
	if (!existsSync(directory)) return [];
	return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
		const path = join(directory, entry.name);
		return entry.isDirectory() ? filesUnder(path) : [path];
	});
}

function targetFile(urlPath) {
	const clean = decodeURIComponent(urlPath.split("?")[0]);
	const candidates = [
		join(distRoot, clean, "index.html"),
		join(distRoot, clean, "index.htm"),
		join(distRoot, clean.endsWith("/") ? clean.slice(0, -1) : clean),
		join(distRoot, clean),
	];
	return candidates.find(
		(candidate) => candidate.endsWith(".html") && existsSync(candidate),
	);
}

const htmlFiles = filesUnder(distRoot).filter((path) => path.endsWith(".html"));
const failures = [];
const fragmentLinks = [];

for (const file of htmlFiles) {
	const html = readFileSync(file, "utf8");
	const relative = file.slice(distRoot.length + 1);
	for (const match of html.matchAll(/href="([^"]+)"/g)) {
		const href = match[1];
		if (!href.startsWith("/docs/")) continue;
		const [pathPart, fragment] = href.split("#");
		const target = targetFile(pathPart);
		if (!target) {
			failures.push({
				file: relative,
				message: `link "${href}" targets a page that does not exist in dist`,
			});
			continue;
		}
		if (!fragment) continue;
		fragmentLinks.push({ file: relative, href, fragment, target });
	}
}

for (const { file, href, fragment, target } of fragmentLinks) {
	const targetHtml = readFileSync(target, "utf8");
	if (!targetHtml.includes(`id="${fragment}"`)) {
		failures.push({
			file,
			message: `link "${href}" targets fragment #${fragment}, which has no id on ${target.slice(distRoot.length + 1)}`,
		});
	}
}

if (failures.length > 0) {
	for (const failure of failures) {
		console.error(`${failure.file}: ${failure.message}`);
	}
	process.exitCode = 1;
} else {
	console.log(
		`Checked ${htmlFiles.length} pages and ${fragmentLinks.length} in-page /docs/ fragment links against dist/.`,
	);
}
