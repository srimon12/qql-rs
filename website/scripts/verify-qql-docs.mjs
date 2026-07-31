import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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
const wasmPackCache = join(
  websiteRoot,
  "node_modules",
  ".cache",
  "qql-docs-wasm-pack",
);
const require = createRequire(import.meta.url);

// True when executed directly (`node scripts/verify-qql-docs.mjs`), false when
// the module is imported (e.g. by the raw-fence regression test), so helpers
// can be exercised without triggering a wasm-pack build.
const isMain =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;

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

/**
 * Raw QQL fences bypass the executable contract of `{% qqlExample %}`. Walk a
 * Markdoc source line-by-line, masking `qqlExample` blocks, and report any
 * bare ```qql or ```sql fence left over. Fence languages are matched
 * case-insensitively (```QQL and ```SQL are raw fences too), but the message
 * preserves the fence spelling actually used. Line numbers are preserved
 * because masked regions keep their newlines.
 */
export function rawFenceFailures(source, file) {
  const masked = source.replace(
    /{%\s*qqlExample[\s\S]*?{%\s*\/qqlExample\s*%}/g,
    (block) => block.replace(/[^\n]/g, " "),
  );
  const failures = [];
  let fenceLanguage = null;
  masked.split("\n").forEach((line, index) => {
    const match = line.match(/^\s*```([a-zA-Z0-9_-]*)\s*$/);
    if (match) {
      fenceLanguage = fenceLanguage === null ? match[1] : null;
      return;
    }
    const language = fenceLanguage?.toLowerCase();
    if (language === "qql" || language === "sql") {
      failures.push({
        file,
        line: index + 1,
        message:
          `bare \`\`\`${fenceLanguage} fence outside qqlExample; ` +
          "wrap runnable QQL in {% qqlExample %} or use a text/ebnf fence",
      });
    }
  });
  return failures;
}

/**
 * Plan verification: `analyze` compiles every statement to a route. When any
 * statement fails to plan, its route is dropped, so a routes/statements count
 * mismatch surfaces plan-level regressions that `parse` alone cannot see.
 */
function planFailures(wasm, query, relative, line) {
  try {
    const info = wasm.analyze(query);
    if (info?.valid === false) {
      return [{
        file: relative,
        line,
        message: `plan verification failed: ${info.error?.code ?? "unknown"} — ${info.error?.message ?? "no message"}`,
      }];
    }
    const expected = info?.statements_count ?? 1;
    const routes = info?.routes?.length ?? 0;
    if (routes !== expected) {
      return [{
        file: relative,
        line,
        message: `plan verification failed: ${routes}/${expected} statements produced a route`,
      }];
    }
    return [];
  } catch (error) {
    return [{
      file: relative,
      line,
      message: `plan verification threw: ${error instanceof Error ? error.message : String(error)}`,
    }];
  }
}

if (isMain) {
  const wasmOut = mkdtempSync(join(tmpdir(), "qql-docs-wasm-"));
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
        failures.push(...rawFenceFailures(source, file));
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
            failures.push(...planFailures(wasm, query, file, line));
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
            failures.push(...planFailures(wasm, query, file, line));
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
          failures.push(...planFailures(wasm, query, file, 1));
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
}
