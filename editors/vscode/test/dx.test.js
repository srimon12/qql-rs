"use strict";

/**
 * Contract + wiring tests for DX items 1-8.
 *
 * WASM behavior the extension depends on is asserted against the bundle
 * (Stmt injectFilter round-trip, fail-closed codes, placeholder token
 * shapes); everything else is static source wiring (commands, providers,
 * settings, docs) in the style of completions.test.js.
 */

const { test } = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const wasm = require(path.join(__dirname, "..", "wasm", "qql_wasm.js"));
const root = path.join(__dirname, "..");
const src = (p) => fs.readFileSync(path.join(root, "src", p), "utf8");

// ── WASM contracts ─────────────────────────────────────────────

test("Stmt injectFilter round-trips through canonical QQL", () => {
  const stmt = new wasm.Stmt("QUERY TEXT 'x' FROM docs USING dense LIMIT 5;");
  stmt.injectFilter("tenant", "=", "acme");
  const out = stmt.toString();
  assert.match(out, /WHERE tenant = 'acme'/, `filter missing in:\n${out}`);
  assert.strictEqual(wasm.analyze(out).valid, true, "injected output must re-parse");
});

test("Stmt injectFilter is fail-closed on DDL", () => {
  const stmt = new wasm.Stmt("SHOW COLLECTIONS;");
  assert.throws(() => stmt.injectFilter("t", "=", "a"), /QQL-VALIDATION-FILTER-INJECT/);
});

test("Stmt injectFilter rejects unsupported operators", () => {
  const stmt = new wasm.Stmt("QUERY TEXT 'x' FROM docs USING dense LIMIT 5;");
  assert.throws(() => stmt.injectFilter("t", "!=", "a"), /QQL-VALIDATION-FILTER-INJECT/);
  const ok = new wasm.Stmt("QUERY TEXT 'x' FROM docs USING dense LIMIT 5;");
  ok.injectFilter("n", ">=", 3);
  assert.match(ok.toString(), /n >= 3/);
});

test("placeholder token shapes for exact-range diagnostics", () => {
  const kinds = wasm.tokenize("QUERY TEXT :q FROM docs USING dense LIMIT ?;");
  const colon = kinds.findIndex((t) => t.kind === "COLON" && t.text === ":");
  assert.ok(colon >= 0, "expected a COLON token");
  assert.strictEqual(kinds[colon + 1].kind, "IDENTIFIER");
  assert.strictEqual(kinds[colon + 1].text, "q");
  assert.ok(
    kinds.some((t) => t.text === "?"),
    "expected a ? token",
  );
});

test("duplicate WAIT and positional-missing error shapes", () => {
  const dup = wasm.analyze("DELETE FROM docs WHERE a = 1 WAIT true WAIT false;");
  assert.strictEqual(dup.error.code, "QQL-PARSE-DUPLICATE-CLAUSE");
  assert.ok(dup.error.start != null && dup.error.end > dup.error.start);
  const missing = wasm.analyze("QUERY TEXT ? FROM docs USING dense LIMIT ?;");
  assert.strictEqual(missing.error.code, "QQL-BIND-MISSING-PARAM");
  assert.match(missing.error.message, /positional parameter '\?\d+'/);
});

// ── Wiring ─────────────────────────────────────────────────────

test("package.json contributes DX commands, menus, and profile settings", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const ids = pkg.contributes.commands.map((c) => c.command);
  for (const id of [
    "qql.run",
    "qql.addFilter",
    "qql.validateWorkspace",
    "qql.showTokens",
    "qql.selectParamsProfile",
  ]) {
    assert.ok(ids.includes(id), `missing command ${id}`);
  }
  const palette = pkg.contributes.menus.commandPalette.map((m) => m.command);
  const submenu = pkg.contributes.menus["qql.submenu"].map((m) => m.command);
  for (const id of ["qql.run", "qql.addFilter", "qql.validateWorkspace", "qql.showTokens"]) {
    assert.ok(palette.includes(id), `${id} missing from commandPalette`);
    assert.ok(submenu.includes(id), `${id} missing from qql.submenu`);
  }
  const props = pkg.contributes.configuration.properties;
  assert.ok(props["qql.paramProfiles"], "missing qql.paramProfiles");
  assert.ok(props["qql.activeProfile"] !== undefined, "missing qql.activeProfile");
});

test("commands.ts registers the DX commands on the shared channel", () => {
  const commands = src("ui/commands.ts");
  for (const id of [
    "qql.showTokens",
    "qql.run",
    "qql.addFilter",
    "qql.validateWorkspace",
    "qql.selectParamsProfile",
  ]) {
    assert.ok(commands.includes(`"${id}"`), `commands.ts must register ${id}`);
  }
  assert.match(commands, /tokenizeQql/, "showTokens must reuse tokenize");
  assert.match(commands, /injectFilterQql/, "addFilter must reuse WASM injectFilter");
  assert.match(commands, /qql-workspace/, "workspace validation needs its own collection");
  assert.match(commands, /fetch\(/, "run must POST via fetch");
});

test("wasm.ts exposes injectFilterQql via Stmt", () => {
  assert.match(src("core/wasm.ts"), /injectFilterQql/, "missing injectFilterQql bridge");
});

test("codeActions provider covers dup WAIT + missing params", () => {
  const provider = src("providers/codeActions.ts");
  assert.match(provider, /QQL-PARSE-DUPLICATE-CLAUSE/);
  assert.match(provider, /QQL-BIND-MISSING-PARAM/);
  assert.match(provider, /CodeActionKind\.QuickFix/);
  assert.match(
    src("extension.ts"),
    /registerCodeActionsProvider/,
    "extension must register the provider",
  );
});

test("diagnostics use exact bind ranges and error-code links", () => {
  const diagnostics = src("providers/diagnostics.ts");
  assert.match(diagnostics, /refineBindRange/, "missing placeholder range refinement");
  assert.match(diagnostics, /tokenizeQql/, "refinement must reuse tokenize");
  assert.match(diagnostics, /error-codes/, "diagnostic code must link the error reference");
  assert.match(diagnostics, /target: vscode\.Uri\.parse/, "code link must carry a URI target");
});

test("hover leads with the compiled route preview", () => {
  const hover = src("providers/hover.ts");
  assert.match(hover, /compileQql/, "hover must reuse compile for the preview");
  assert.match(hover, /LIMIT \$/);
});

test("params profiles build on params.ts with picker + indicators", () => {
  const params = src("core/params.ts");
  assert.match(params, /getActiveProfileName/);
  assert.match(params, /getParamProfiles/);
  assert.match(params, /setActiveProfile/);
  assert.match(params, /describeParamsSource/);
  assert.match(src("ui/statusBar.ts"), /selectParamsProfile/);
  assert.match(src("ui/statusBar.ts"), /suffix/, "status bar must show the active profile");
  assert.match(src("providers/codelens.ts"), /paramsSource/);
});

test("README documents the DX set", () => {
  const readme = fs.readFileSync(path.join(root, "README.md"), "utf8");
  for (const needle of [
    "Show Tokens",
    "Run Statement",
    "Add Tenant Filter",
    "Validate Workspace",
    "Select Params Profile",
    "qql.paramProfiles",
    "qql.activeProfile",
    "Quick Fixes",
    "error-codes",
  ]) {
    assert.ok(readme.includes(needle), `README missing "${needle}"`);
  }
});
