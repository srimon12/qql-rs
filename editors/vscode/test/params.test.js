/**
 * Contract tests for the `:name` / `?` placeholder path the extension relies
 * on (`src/core/wasm.ts` + `src/core/params.ts`).
 *
 * The WASM surface is the source of truth: `bind` substitutes, `compile`
 * binds before parsing, and `analyze` without params fails with `QQL-BIND-*`
 * (which is why the extension retries analysis with the bound text when the
 * `-- qql-params` header or `qql.params` setting is present).
 */

const { test } = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const wasm = require(path.join(__dirname, "..", "wasm", "qql_wasm.js"));
const root = path.join(__dirname, "..");

test("bind substitutes named placeholders from an object", () => {
  assert.strictEqual(
    wasm.bind("QUERY TEXT :q FROM docs USING dense LIMIT :n", { q: "hi", n: 10 }),
    "QUERY TEXT 'hi' FROM docs USING dense LIMIT 10"
  );
});

test("bind substitutes positional placeholders from an array", () => {
  assert.strictEqual(
    wasm.bind("QUERY TEXT ? FROM docs USING dense LIMIT ?", ["hi", 10]),
    "QUERY TEXT 'hi' FROM docs USING dense LIMIT 10"
  );
});

test("compile binds params before parsing", () => {
  const route = wasm.compile("QUERY TEXT :q FROM docs USING dense LIMIT 10", { q: "hi" });
  assert.strictEqual(route.method, "POST");
  assert.strictEqual(route.path, "/collections/docs/points/query");
});

test("analyze without params reports a QQL-BIND-* error", () => {
  const result = wasm.analyze("QUERY TEXT :q FROM docs USING dense LIMIT 10");
  assert.strictEqual(result.valid, false);
  assert.ok(result.error, "expected an error");
  assert.ok(
    result.error.code.startsWith("QQL-BIND-"),
    `expected QQL-BIND-*, got ${result.error.code}`
  );
});

test("analyze accepts the bound text", () => {
  const bound = wasm.bind("QUERY TEXT :q FROM docs USING dense LIMIT 10", { q: "hi" });
  const result = wasm.analyze(bound);
  assert.strictEqual(result.valid, true);
});

test("params supply path is documented and wired", () => {
  const params = fs.readFileSync(path.join(root, "src", "core", "params.ts"), "utf8");
  assert.match(params, /qql-params/, "params.ts must read the -- qql-params header");
  assert.match(params, /getDocumentParams/, "params.ts must export getDocumentParams");
  const wasmBridge = fs.readFileSync(path.join(root, "src", "core", "wasm.ts"), "utf8");
  assert.match(wasmBridge, /bindQql/, "wasm.ts must expose bindQql");
  assert.match(wasmBridge, /QQL-BIND-/, "wasm.ts must retry only on QQL-BIND-* errors");
  const readme = fs.readFileSync(path.join(root, "README.md"), "utf8");
  assert.match(readme, /qql-params/, "README must document the params header");
  assert.match(readme, /qql\.params/, "README must document the qql.params setting");
});
