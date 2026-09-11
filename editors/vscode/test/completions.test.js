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
const completions = fs.readFileSync(
  path.join(root, "src", "providers", "completions.ts"),
  "utf8",
);
const keywords = fs.readFileSync(
  path.join(root, "src", "keywords.generated.ts"),
  "utf8",
);

test("snippet count matches the documented claim (README: 36)", () => {
  const labels = [...completions.matchAll(/^\s*label: "([^"]+)",$/gm)].map((m) => m[1]);
  assert.strictEqual(labels.length, 36, "expected exactly 36 snippets");
  assert.strictEqual(new Set(labels).size, labels.length, "snippet labels must be unique");
});

test("snippet insertText values are well-formed", () => {
  // No literal backslash-n (double backslash + n) anywhere — that inserts a
  // visible "\n" into the user's document instead of a newline.
  assert.doesNotMatch(completions, /\\\\n/, "found literal \\\\n in providers/completions.ts");

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
  const snippetsEnd = completions.indexOf("// Contextual follow-ups");
  const snippetBlock = completions.slice(0, snippetsEnd);
  const insertTextCount = (snippetBlock.match(/\binsertText:/g) || []).length;
  const detailCount = (snippetBlock.match(/\bdetail:/g) || []).length;
  // One declaration belongs to the QqlSnippet interface; every concrete
  // snippet contributes exactly one additional property.
  assert.strictEqual(insertTextCount, 37, "every snippet must have insertText");
  assert.strictEqual(detailCount, 37, "every snippet must have a detail");
});

test("1.7 statement starters and follow-ups are registered", () => {
  const statements = fs.readFileSync(
    path.join(root, "src", "core", "statements.ts"),
    "utf8",
  );
  for (const starter of ["FACET", "SET"]) {
    assert.match(completions, new RegExp(`"${starter}",?\\s*$`, "m"), `${starter} must be a statement starter`);
    assert.ok(
      statements.includes(`"${starter}",`),
      `statements.ts splitter must start on ${starter}`,
    );
  }
  assert.match(statements, /pendingSet/, "splitter must refine SET QUOTA");
  assert.match(statements, /SET QUOTA/, "splitter must label SET QUOTA");
  assert.match(statements, /SHOW QUOTAS/, "splitter must label SHOW QUOTAS");
  for (const key of ["WAIT:", "SET:"]) {
    assert.match(completions, new RegExp(`^\\s*${key}`, "m"), `AFTER map must define ${key}`);
  }
  const afterStart = completions.indexOf("const AFTER");
  const startersStart = completions.indexOf("const STATEMENT_STARTERS");
  const afterBlock = completions.slice(afterStart, startersStart);
  for (const follow of ["SPARSE", "QUANTIZATION", "OPTIMIZERS", "QUOTAS"]) {
    assert.ok(afterBlock.includes(`label: "${follow}"`), `AFTER map must suggest ${follow}`);
  }
  const clauseBlock = completions.slice(completions.indexOf("const CLAUSE_KEYWORDS"));
  assert.ok(clauseBlock.includes('"WAIT"'), "WAIT must be a clause keyword");
});

test("1.7 snippets exist in completions", () => {
  for (const label of [
    "ALTER VECTOR DIFF",
    "ALTER SPARSE DIFF",
    "ALTER HNSW",
    "SHARD NUMERIC",
    "QUERY WITH PARAMS",
    "CREATE SHARD KEY NUMERIC",
  ]) {
    assert.ok(completions.includes(`label: "${label}"`), `missing snippet ${label}`);
  }
});

test("snippets/qql.json mirrors the 1.7 additions", () => {
  const snippets = JSON.parse(
    fs.readFileSync(path.join(root, "snippets", "qql.json"), "utf8"),
  );
  for (const name of [
    "Alter vector diff",
    "Alter sparse diff",
    "Alter HNSW",
    "Shard numeric routing",
    "Query with params",
    "Create shard key numeric",
  ]) {
    assert.ok(snippets[name], `snippets/qql.json missing "${name}"`);
    const body = snippets[name].body.join("\n");
    assert.doesNotMatch(body, /\\\\n/, `"${name}" body has a literal \\\\n`);
  }
  const allBodies = Object.values(snippets).map((s) => s.body.join("\n")).join("\n");
  assert.match(allBodies, /WITH VECTOR \$\{2:dense\}/, "vector-diff snippet missing");
  assert.match(allBodies, /WITH SPARSE/, "sparse-diff snippet missing");
  assert.match(allBodies, /SHARD \$\{6:101\}/, "numeric SHARD snippet missing");
  assert.match(allBodies, /qql-params/, "params-header snippet missing");
  assert.match(allBodies, /WAIT \$\{/, "WAIT snippet missing");
});

test("qql.params setting is contributed", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const params = pkg.contributes.configuration.properties["qql.params"];
  assert.ok(params, "package.json must contribute qql.params");
});

test("diagnostics hint the new 1.7 error codes", () => {
  const diagnostics = fs.readFileSync(
    path.join(root, "src", "providers", "diagnostics.ts"),
    "utf8",
  );
  for (const code of [
    "QQL-PARSE-VECTOR-DIFF",
    "QQL-PLAN-VECTOR-DIFF",
    "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF",
    "QQL-EDGE-UNSUPPORTED-SPARSE-DIFF",
    "QQL-PARSE-DUPLICATE-CLAUSE",
    "QQL-UNKNOWN-VECTOR",
  ]) {
    assert.ok(diagnostics.includes(code), `diagnostics must hint ${code}`);
  }
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
