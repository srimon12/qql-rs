/**
 * Build script: bundles the TypeScript sources and WASM binary
 * into dist/ for VS Code extension packaging.
 *
 * Usage: node scripts/build.mjs
 */

import * as esbuild from "esbuild";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const dist = path.join(root, "dist");
const src = path.join(root, "src");

// Clean
if (fs.existsSync(dist)) {
  fs.rmSync(dist, { recursive: true });
}
fs.mkdirSync(dist, { recursive: true });

// Build extension
await esbuild.build({
  entryPoints: [path.join(src, "extension.ts")],
  bundle: true,
  outfile: path.join(dist, "extension.js"),
  platform: "node",
  target: "node18",
  format: "cjs",
  external: ["vscode", "qql-wasm"],
  sourcemap: true,
  minify: false,
  logLevel: "info",
});

// Copy WASM binary if it exists (built separately from qql-wasm crate)
const wasmSrc = path.join(root, "..", "..", "crates", "qql-wasm", "pkg-node", "qql_wasm_bg.wasm");
if (fs.existsSync(wasmSrc)) {
  fs.copyFileSync(wasmSrc, path.join(dist, "qql_wasm_bg.wasm"));
  console.log("Copied WASM binary from qql-wasm/pkg-node/");
} else {
  console.warn("WASM binary not found — run `wasm-pack build crates/qql-wasm --target nodejs` first");
  console.warn("Diagnostics will not work until the WASM binary is bundled.");
}

console.log("Build complete.");
