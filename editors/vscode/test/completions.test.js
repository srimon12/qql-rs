"use strict";

/**
 * Static well-formedness tests for the QQL VS Code extension sources.
 *
 * These run with plain Node (no TypeScript toolchain) by reading the source
 * files as text. They guard the snippet/keyword claims in README.md and the
 * website, and specifically the "QUERY IMAGE" literal-\n bug (previously the
 * snippet inserted a backslash-n instead of a newline).
 */

const { test } = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..");
const completions = fs.readFileSync(path.join(root, "src", "completions.ts"), "utf8");
const keywords = fs.readFileSync(path.join(root, "src", "keywords.generated.ts"), "utf8");

test("snippet count matches the documented claim (README: 27)", () => {
  const labels = [...completions.matchAll(/^\s*label: "([^"]+)",$/gm)].map((m) => m[1]);
  assert.strictEqual(labels.length, 27, "expected exactly 27 snippets");
  assert.strictEqual(new Set(labels).size, labels.length, "snippet labels must be unique");
});

test("snippet insertText values are well-formed", () => {
  // No literal backslash-n (double backslash + n) anywhere — that inserts a
  // visible "\n" into the user's document instead of a newline.
  assert.doesNotMatch(completions, /\\\\n/, "found literal \\\\n in completions.ts");

  // The QUERY IMAGE snippet must use a real \n escape before "  FROM".
  assert.match(
    completions,
    /QUERY IMAGE '\$\{1:[^']*}' MODEL '\$\{2:clip-vit\}'\\n  FROM \$\{3:collection\}/,
    "QUERY IMAGE snippet must break lines with \\n escapes",
  );

  // Every insertText line must terminate its string cleanly (no trailing
  // dangling backslash) and every snippet entry must carry a detail.
  for (const m of completions.matchAll(/insertText: "([^"]*)"(?:\s*\+|\s*,)/g)) {
    assert.doesNotMatch(m[1], /\\$/, "insertText fragment must not end in a backslash");
  }
  const insertTextCount = (completions.match(/insertText: "/g) || []).length;
  const detailCount = (completions.match(/detail: "/g) || []).length;
  assert.strictEqual(insertTextCount, 27, "every snippet must have insertText");
  assert.strictEqual(detailCount, 27, "every snippet must have a detail");
});

test("keyword count supports the '130+' claim", () => {
  const words = [...keywords.matchAll(/^\s*"([A-Z0-9_]+)",$/gm)].map((m) => m[1]);
  assert.ok(words.length >= 130, `expected 130+ keywords, got ${words.length}`);
  assert.strictEqual(new Set(words).size, words.length, "keywords must be unique");
});

test("extension main entry points at the tsc output (out/)", () => {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(root, "package.json"), "utf8"),
  );
  assert.strictEqual(pkg.main, "./out/extension.js");
  // No dead esbuild glue: the build script must not exist anymore.
  assert.ok(
    !(pkg.scripts || {}).build,
    "package.json must not keep the dead esbuild build script",
  );
  assert.ok(
    !(pkg.devDependencies || {}).esbuild,
    "esbuild devDependency must be removed with the dead build script",
  );
  assert.ok(
    !fs.existsSync(path.join(root, "scripts", "build.mjs")),
    "scripts/build.mjs must be deleted",
  );
});
