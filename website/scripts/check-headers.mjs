#!/usr/bin/env node
// Guards the Cloudflare `_headers` CSP invariant: browsers enforce ALL
// received Content-Security-Policy headers (intersection), and Cloudflare
// sends headers from EVERY matching rule — so a CSP on `/*` stacks on top of
// a section rule and the stricter one wins everywhere. That exact stacking
// once blocked the playground from reaching user-configured Qdrant/embedder
// origins (localhost, LAN, custom domains) despite `/playground/*` allowing
// them. The invariant: every built HTML route must match exactly ONE rule
// carrying a CSP, and the playground's CSP must keep `http:` in connect-src.
// Usage: node scripts/check-headers.mjs (runs post-build in `pnpm check`).

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const headersPath = join(root, "public", "_headers");
const dist = join(root, "dist");

// 404.html is served for unmatched paths, where no section rule can target;
// it is the only HTML allowed zero CSP-bearing matches.
const ZERO_CSP_OK = new Set(["404.html"]);

function parseRules(text) {
	const rules = [];
	let current = null;
	for (const line of text.split("\n")) {
		if (!line.trim() || line.trimStart().startsWith("#")) continue;
		if (!/^\s/.test(line)) {
			current = { path: line.trim(), headers: new Map() };
			rules.push(current);
		} else if (current) {
			const index = line.indexOf(":");
			if (index > 0) {
				current.headers.set(
					line.slice(0, index).trim().toLowerCase(),
					line.slice(index + 1).trim(),
				);
			}
		}
	}
	return rules;
}

function matches(rulePath, requestPath) {
	if (rulePath.endsWith("/*")) {
		const prefix = rulePath.slice(0, -2);
		return requestPath === prefix || requestPath.startsWith(`${prefix}/`);
	}
	return rulePath === requestPath;
}

function htmlRoutes(dir, base = "/") {
	const routes = [];
	for (const entry of readdirSync(dir)) {
		const full = join(dir, entry);
		if (statSync(full).isDirectory()) {
			routes.push(...htmlRoutes(full, `${base}${entry}/`));
		} else if (entry.endsWith(".html")) {
			const name = entry === "index.html" ? base : `${base}${entry}`;
			routes.push(name);
		}
	}
	return routes;
}

const rules = parseRules(readFileSync(headersPath, "utf8"));
const failures = [];

for (const route of htmlRoutes(dist)) {
	const matched = rules.filter((rule) => matches(rule.path, route));
	const csp = matched.filter((rule) =>
		rule.headers.has("content-security-policy"),
	);
	const file = route === "/" ? "index.html" : route.slice(1);
	if (csp.length === 0 && !ZERO_CSP_OK.has(file)) {
		failures.push(`${route}: no CSP-bearing rule matches (add a section rule)`);
	} else if (csp.length > 1) {
		failures.push(
			`${route}: ${csp.length} CSP headers would stack (${csp.map((r) => r.path).join(", ")}) — browsers enforce ALL of them`,
		);
	}
	if (route === "/playground/") {
		const policy = csp[0]?.headers.get("content-security-policy") ?? "";
		const connect = /connect-src([^;]*)/.exec(policy)?.[1] ?? "";
		if (!/(^|\s)http:(\s|$)/.test(connect)) {
			failures.push(
				"/playground/: connect-src lost `http:` (localhost Qdrant blocked)",
			);
		}
	}
}

if (failures.length > 0) {
	console.error("check-headers: FAIL");
	for (const failure of failures) console.error(`  - ${failure}`);
	process.exit(1);
}
console.log("check-headers: OK");
