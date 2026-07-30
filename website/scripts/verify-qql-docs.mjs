import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const websiteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(websiteRoot, "..");
const docsRoot = join(websiteRoot, "src", "content", "docs", "docs");
const pagesRoot = join(websiteRoot, "src", "pages");
const validFixturesRoot = join(
  workspaceRoot,
  "language",
  "v1",
  "fixtures",
  "valid",
);
const wasmOut = mkdtempSync(join(tmpdir(), "qql-docs-wasm-"));
const wasmPackCache = join(
  websiteRoot,
  "node_modules",
  ".cache",
  "qql-docs-wasm-pack",
);
const require = createRequire(import.meta.url);

function filesUnder(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesUnder(path) : [path];
  });
}

function sourceLocation(source, offset) {
  const before = source.slice(0, offset);
  return before.split("\n").length;
}

function qqlExampleSource(body) {
  const fence = body.match(/^\s*```(?:qql|sql)?\s*\n([\s\S]*?)\n\s*```\s*$/i);
  const source = fence?.[1] ?? body;
  return source.replace(/^ {1,3}/gm, "").trim();
}

try {
  const build = spawnSync(
    "wasm-pack",
    [
      "build",
      join(workspaceRoot, "crates", "qql-wasm"),
      "--release",
      "--target",
      "nodejs",
      "--out-dir",
      wasmOut,
    ],
    {
      cwd: workspaceRoot,
      encoding: "utf8",
      stdio: "pipe",
      env: {
        ...process.env,
        RUSTC_WRAPPER: "",
        WASM_PACK_CACHE: wasmPackCache,
      },
    },
  );

  if (build.status !== 0) {
    process.stderr.write(build.stdout);
    process.stderr.write(build.stderr);
    process.exitCode = build.status ?? 1;
  } else {
    const wasm = require(join(wasmOut, "qql_wasm.js"));
    const failures = [];
    let examples = 0;
    let statements = 0;
    let fixtureGroups = 0;
    let fixtureStatements = 0;
    const tagPattern =
      /{%\s*qqlExample(?:\s+[^%]*?)?%}([\s\S]*?){%\s*\/qqlExample\s*%}/g;

    for (const file of filesUnder(docsRoot).filter((path) =>
      path.endsWith(".mdoc"),
    )) {
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(tagPattern)) {
        examples += 1;
        if (!match[1].trimStart().startsWith("```")) {
          failures.push({
            file,
            line: sourceLocation(source, match.index),
            message: "qqlExample content must use a fenced SQL block to preserve formatting",
          });
          continue;
        }
        const query = qqlExampleSource(match[1]);
        const line = sourceLocation(source, match.index);
        if (!query) {
          failures.push({ file, line, message: "empty qqlExample" });
          continue;
        }

        try {
          const ast = wasm.parse(query);
          statements += Array.isArray(ast) ? ast.length : 1;
        } catch (error) {
          failures.push({
            file,
            line,
            message: error instanceof Error ? error.message : String(error),
          });
        }
      }
    }

    const astroPattern =
      /const\s+verifiedQql\w*\s*=\s*String\.raw`([\s\S]*?)`;/g;
    for (const file of filesUnder(pagesRoot).filter((path) =>
      path.endsWith(".astro"),
    )) {
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(astroPattern)) {
        examples += 1;
        const query = match[1].trim();
        const line = sourceLocation(source, match.index);
        try {
          const ast = wasm.parse(query);
          statements += Array.isArray(ast) ? ast.length : 1;
        } catch (error) {
          failures.push({
            file,
            line,
            message: error instanceof Error ? error.message : String(error),
          });
        }
      }
    }

    for (const file of filesUnder(validFixturesRoot)
      .filter((path) => path.endsWith(".qql"))
      .sort()) {
      fixtureGroups += 1;
      const query = readFileSync(file, "utf8").trim();
      try {
        const ast = wasm.parse(query);
        fixtureStatements += Array.isArray(ast) ? ast.length : 1;
      } catch (error) {
        failures.push({
          file,
          line: 1,
          message: error instanceof Error ? error.message : String(error),
        });
      }
    }

    if (failures.length > 0) {
      for (const failure of failures) {
        const relative = failure.file.slice(websiteRoot.length + 1);
        console.error(`${relative}:${failure.line}: ${failure.message}`);
      }
      process.exitCode = 1;
    } else {
      console.log(
        `Verified ${examples} documentation examples (${statements} statements) and ${fixtureGroups} playground fixture groups (${fixtureStatements} statements) with freshly built qql-wasm.`,
      );
    }
  }
} finally {
  rmSync(wasmOut, { recursive: true, force: true });
}
