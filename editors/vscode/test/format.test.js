"use strict";

/**
 * Corpus-driven contract tests for the checked-in WASM bundle.
 *
 * The grammar + Rust parser are the single source of truth. `qql-conformance
 * generate` projects that truth into two snapshot sets, and this test holds
 * the bundled WASM against both of them:
 *
 * 1. fixtures/formatted/*.txt  — canonical format of every valid fixture.
 *    The bundle's `formatQuery` (used by Format Document) must reproduce it
 *    byte-for-byte.
 * 2. fixtures/invalid/*.qql    — every `-- @case` program must be rejected,
 *    with the exact `-- @error` code the conformance gate demands.
 *
 * A stale bundle (rebuilt-never, e.g. after a grammar change) diverges on the
 * first fixture that exercises the new surface — no hand-maintained probe
 * list can forget it.
 */

const { test } = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const wasm = require(path.join(__dirname, "..", "wasm", "qql_wasm.js"));

// editors/vscode/test → repository root
const repoRoot = path.join(__dirname, "..", "..", "..");
const languageDir = path.join(repoRoot, "language", "v1");
const validDir = path.join(languageDir, "fixtures", "valid");
const invalidDir = path.join(languageDir, "fixtures", "invalid");
const formattedDir = path.join(languageDir, "fixtures", "formatted");

/** Recursively collect files with the given extension (sorted). */
function filesWithExtension(dir, ext, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      filesWithExtension(p, ext, out);
    } else if (path.extname(entry.name) === ext) {
      out.push(p);
    }
  }
  return out.sort();
}

/**
 * Split an invalid fixture into `-- @case` programs, mirroring
 * qql-conformance's `invalid_cases()` parser.
 */
function invalidCases(source) {
  const cases = [];
  let current = null;
  for (const line of source.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith("-- @case ")) {
      if (current) cases.push(current);
      const name = trimmed.slice("-- @case ".length).trim();
      assert.ok(name, "invalid fixture has an empty @case name");
      current = { name, expectedError: null, source: "" };
      continue;
    }
    if (trimmed.startsWith("-- @error ")) {
      assert.ok(current, "@error must follow an @case marker");
      assert.strictEqual(
        current.expectedError,
        null,
        `case '${current.name}' has multiple @error markers`,
      );
      current.expectedError = trimmed.slice("-- @error ".length).trim();
      continue;
    }
    if (current) {
      current.source += line + "\n";
    } else if (trimmed !== "" && !trimmed.startsWith("--")) {
      assert.fail("invalid fixtures must use `-- @case <name>` markers");
    }
  }
  if (current) cases.push(current);
  for (const c of cases) {
    c.source = c.source.trim();
    assert.ok(c.source, `case '${c.name}' has no QQL source`);
  }
  assert.ok(cases.length > 0, "invalid fixture contains no @case markers");
  return cases;
}

// ── Corpus: canonical format ─────────────────────────────────────

const validFixtures = filesWithExtension(validDir, ".qql");

test("canonical-format goldens exist for every valid fixture", () => {
  assert.ok(validFixtures.length >= 30, `fixture corpus shrunk: ${validFixtures.length}`);
  for (const fixture of validFixtures) {
    const golden = path.join(
      formattedDir,
      path.relative(validDir, fixture).replace(/\.qql$/, ".txt"),
    );
    assert.ok(
      fs.existsSync(golden),
      `missing canonical-format golden for ${path.basename(fixture)}; run cargo run -p qql-conformance -- generate`,
    );
  }
});

test("bundled WASM format matches the canonical golden of every fixture", () => {
  let mismatch = 0;
  for (const fixture of validFixtures) {
    const source = fs.readFileSync(fixture, "utf8");
    const golden = fs.readFileSync(
      path.join(formattedDir, path.relative(validDir, fixture).replace(/\.qql$/, ".txt")),
      "utf8",
    );
    let formatted;
    try {
      formatted = wasm.formatQuery(source);
    } catch (err) {
      assert.fail(`${path.basename(fixture)}: bundled WASM failed to format: ${err}`);
    }
    if (formatted !== golden) {
      mismatch++;
      // Show the first divergent fixture instead of 40 diffs.
      assert.fail(
        `${path.basename(fixture)}: bundled WASM format diverges from the canonical golden — the checked-in wasm/ bundle is stale; rebuild with wasm-pack build crates/qql-wasm --release --target nodejs --out-dir ../../editors/vscode/wasm`,
      );
    }
  }
  assert.strictEqual(mismatch, 0);
});

// ── Corpus: rejections ───────────────────────────────────────────

test("bundled WASM rejects every invalid case with the expected error code", () => {
  let checked = 0;
  for (const fixture of filesWithExtension(invalidDir, ".qql")) {
    const source = fs.readFileSync(fixture, "utf8");
    for (const testCase of invalidCases(source)) {
      const analysis = wasm.analyze(testCase.source);
      assert.strictEqual(
        analysis.valid,
        false,
        `${path.basename(fixture)} [${testCase.name}]: bundled WASM accepted an invalid program`,
      );
      assert.ok(analysis.error, `${path.basename(fixture)} [${testCase.name}]: no error reported`);
      if (testCase.expectedError) {
        assert.strictEqual(
          analysis.error.code,
          testCase.expectedError,
          `${path.basename(fixture)} [${testCase.name}]: expected error ${testCase.expectedError}, got ${analysis.error.code}`,
        );
      }
      checked++;
    }
  }
  assert.ok(checked >= 50, `invalid corpus shrunk: only ${checked} cases checked`);
});

// ── Smoke tests (kept cheap; corpus covers the grammar surface) ──

test("bundled WASM exposes formatQuery", () => {
  assert.strictEqual(typeof wasm.formatQuery, "function");
});

test("formatQuery normalizes whitespace and keyword casing", () => {
  assert.strictEqual(
    wasm.formatQuery("  QUERY   'hello'   from docs  LIMIT 10 ;"),
    "QUERY 'hello' FROM docs LIMIT 10;\n",
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
