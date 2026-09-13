#!/usr/bin/env node
// Guards the authored-CSS budget (website/packages/ui/README.md):
//   1. exactly one non-excluded stylesheet lives in packages/ui/src/styles;
//   2. that file stays <= 500 lines;
//   3. no other authored .css file exists under src/ or packages/*;
//   4. the wiring file src/styles/global.css stays <= 15 lines.
// Usage: node scripts/check-css-budget.mjs

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BUDGET = 500;
const WIRING_BUDGET = 15;
const EXCLUDED = new Set(["ec-theme.css"]);
const IGNORED_DIRS = new Set([
	"node_modules",
	"dist",
	".astro",
	".wasm",
	"public",
]);
const ALLOWED = new Set([
	"packages/ui/src/styles/styles.css",
	"packages/ui/src/styles/ec-theme.css",
	"src/styles/global.css",
]);

function countLines(path) {
	const content = readFileSync(path, "utf8").replace(/\n$/, "");
	return content.split("\n").length;
}

function walk(dir, out = []) {
	if (!existsSync(dir)) return out;
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (entry.isDirectory()) {
			if (!IGNORED_DIRS.has(entry.name)) walk(join(dir, entry.name), out);
		} else if (entry.name.endsWith(".css")) {
			out.push(join(dir, entry.name));
		}
	}
	return out;
}

const failures = [];

const stylesDir = join(root, "packages", "ui", "src", "styles");
const authored = readdirSync(stylesDir)
	.filter((name) => name.endsWith(".css") && !EXCLUDED.has(name))
	.sort();
if (authored.length !== 1 || authored[0] !== "styles.css") {
	failures.push(
		`expected exactly one authored sheet (styles.css) in packages/ui/src/styles, found: ${authored.join(", ") || "none"}`,
	);
} else {
	const lines = countLines(join(stylesDir, authored[0]));
	if (lines > BUDGET) {
		failures.push(
			`styles.css is ${lines} lines, over the ${BUDGET}-line budget`,
		);
	}
}

for (const file of [
	...walk(join(root, "src")),
	...walk(join(root, "packages")),
]) {
	const rel = relative(root, file).split("\\").join("/");
	if (!ALLOWED.has(rel)) {
		failures.push(
			`${rel} is an authored stylesheet; put it in styles.css or delete it`,
		);
	}
}

const wiring = join(root, "src", "styles", "global.css");
const wiringLines = countLines(wiring);
if (wiringLines > WIRING_BUDGET) {
	failures.push(
		`src/styles/global.css is ${wiringLines} lines, over the ${WIRING_BUDGET}-line wiring budget`,
	);
}

if (failures.length > 0) {
	for (const failure of failures) console.error(`check:css: ${failure}`);
	process.exitCode = 1;
} else {
	console.log(
		`check:css: styles.css ${countLines(join(stylesDir, "styles.css"))}/${BUDGET} lines, global.css ${wiringLines}/${WIRING_BUDGET} lines, one authored sheet.`,
	);
}
