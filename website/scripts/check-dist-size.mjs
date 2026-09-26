#!/usr/bin/env node
// Guards the deployed bundle against Cloudflare Pages' 25 MiB per-file limit.
// The playground embedder self-hosts the onnxruntime wasm that Vite copies out
// of @huggingface/transformers; an upstream bump once pushed it to 25.6 MiB
// (onnxruntime-web 1.31 asyncify) and every `wrangler pages deploy` failed
// after the site had already built. Catch that here, before upload.
//
// Usage: node scripts/check-dist-size.mjs [dist-dir]

import { existsSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const distDir = resolve(root, process.argv[2] ?? "dist");
const MIB = 1024 * 1024;
const LIMIT = 25 * MIB;
const LIMIT_LABEL = "25 MiB";

function walk(dir, out = []) {
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) walk(path, out);
		else if (entry.isFile()) out.push(path);
	}
	return out;
}

if (!existsSync(distDir)) {
	console.error(`check-dist-size: FAIL — no build output at ${distDir}`);
	process.exit(1);
}

const oversized = [];
let largest = null;
for (const path of walk(distDir)) {
	const size = statSync(path).size;
	if (size > LIMIT) oversized.push({ path, size });
	else if (!largest || size > largest.size) largest = { path, size };
}

if (oversized.length > 0) {
	console.error("check-dist-size: FAIL");
	for (const { path, size } of oversized) {
		console.error(
			`  - ${relative(root, path)} is ${(size / MIB).toFixed(1)} MiB ` +
				`(Cloudflare Pages limit: ${LIMIT_LABEL} per file; ${((size - LIMIT) / MIB).toFixed(1)} MiB over)`,
		);
	}
	console.error(
		"  Offload or shrink the asset — see the @huggingface/transformers pin in package.json.",
	);
	process.exit(1);
}

const largestLabel = largest
	? `${(largest.size / MIB).toFixed(1)} MiB (${relative(root, largest.path)})`
	: "no files";
console.log(`check-dist-size: OK (largest file ${largestLabel})`);
