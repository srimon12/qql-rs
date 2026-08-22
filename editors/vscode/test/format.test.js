"use strict";

/**
 * End-to-end tests for the bundled WASM `formatQuery` used by the QQL
 * Format Document provider. Loads the checked-in Node build directly.
 */

const { test } = require("node:test");
const assert = require("node:assert");
const path = require("node:path");

const wasm = require(path.join(__dirname, "..", "wasm", "qql_wasm.js"));

test("bundled WASM exposes formatQuery", () => {
  assert.strictEqual(typeof wasm.formatQuery, "function");
});

test("formatQuery normalizes whitespace and keyword casing", () => {
  assert.strictEqual(
    wasm.formatQuery("  QUERY   'hello'   from docs  LIMIT 10 ;"),
    "QUERY 'hello' FROM docs LIMIT 10;",
  );
});

test("formatQuery handles multi-statement scripts", () => {
  assert.strictEqual(
    wasm.formatQuery("QUERY 'a' FROM docs LIMIT 5;  count FROM docs WHERE status='x';"),
    "QUERY 'a' FROM docs LIMIT 5;\nCOUNT FROM docs WHERE status = 'x';",
  );
});

test("formatQuery is idempotent", () => {
  const source =
    "WITH c AS (QUERY 'x' USING dense LIMIT 5) QUERY FUSION RRF FROM docs PREFETCH (c) LIMIT 10;";
  const once = wasm.formatQuery(source);
  const twice = wasm.formatQuery(once);
  assert.strictEqual(twice, once);
});

test("formatQuery throws on invalid source", () => {
  assert.throws(() => wasm.formatQuery("QUERY BROKEN"));
});
