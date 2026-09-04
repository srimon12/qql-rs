const nqql = require("./index.js");
const assert = require("assert");
const fs = require("fs");
const path = require("path");
const os = require("os");

// ═══════════════════════════════════════════════════════════════════
//  Reuse project-local fastembed cache (productized via cacheDir too)
// ═══════════════════════════════════════════════════════════════════

const CACHE_CANDIDATES = [
  path.join(__dirname, "..", "..", ".fastembed_cache"),
  path.join(__dirname, "..", "pyqql-edge", ".fastembed_cache"),
  process.env.FASTEMBED_CACHE_DIR,
  process.env.HF_HOME && path.join(process.env.HF_HOME, "hub"),
].filter(Boolean);

let projectCache = null;
for (const dir of CACHE_CANDIDATES) {
  if (fs.existsSync(path.join(dir, "models--Xenova--bge-small-en-v1.5"))) {
    projectCache = dir;
    process.env.HF_HUB_CACHE = dir;
    process.env.FASTEMBED_CACHE_DIR = dir;
    break;
  }
}
console.log("model cache =", projectCache || "(system default — first run may download)");

function tmpDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

function uuid(n) {
  // Deterministic test UUIDs
  return `550e8400-e29b-41d4-a716-44665544${String(n).padStart(4, "0")}`;
}

function assertFails(report, msgPart) {
  assert.strictEqual(report.ok, false, "expected failure report");
  const msg = report.results?.[0]?.message || JSON.stringify(report);
  if (msgPart) {
    assert(
      msg.toLowerCase().includes(String(msgPart).toLowerCase()),
      `expected error containing "${msgPart}", got: ${msg}`,
    );
  }
}

// ═══════════════════════════════════════════════════════════════════
//  1. Parser API
// ═══════════════════════════════════════════════════════════════════

console.log("\n── 1. Parser ──");

const query = "QUERY 'hello' FROM docs LIMIT 10";
const results = nqql.parse(query);
assert(Array.isArray(results));
assert.strictEqual(results.length, 1);
const r0 = results[0];
assert(r0 instanceof nqql.Stmt);
assert.strictEqual(r0.toObject().Query.collection.Explicit, "docs");
assert.strictEqual(r0.toObject().Query.expression.Nearest.input.Text.text, "hello");
console.log("  ✓ parse single query");

const multi = nqql.parse("QUERY 'test' FROM users LIMIT 5; CREATE COLLECTION items");
assert.strictEqual(multi.length, 2);
console.log("  ✓ parse multi-statement script");

const jsonStr = nqql.parseJson(query);
assert(typeof jsonStr === "string");
assert(Array.isArray(JSON.parse(jsonStr)));
console.log("  ✓ parseAllJson");

assert(nqql.isValid("QUERY 'test' FROM docs LIMIT 5"));
assert(!nqql.isValid("garbage not a query"));
console.log("  ✓ isValid");

const tokens = nqql.tokenize("QUERY 'test' FROM docs");
assert(Array.isArray(tokens) && tokens.length > 0);
assert.strictEqual(tokens[0].text, "QUERY");
assert.strictEqual(tokens[0].pos, 0);
assert.strictEqual(tokens[0].end, 5);
assert.strictEqual(tokens[0].len, 5);
console.log("  ✓ tokenize");

// Test Stmt constructor and compileRoute
const constructedStmt = new nqql.Stmt(query);
assert(constructedStmt instanceof nqql.Stmt);
const stmtRoute = constructedStmt.compileRoute();
assert.strictEqual(stmtRoute.stmt_type, "query");
assert.strictEqual(stmtRoute.method, "POST");
assert.strictEqual(stmtRoute.path, "/collections/docs/points/query");
console.log("  ✓ Stmt constructor & compileRoute");

const route = nqql.compileQuery(query);
assert.strictEqual(route.method, "POST");
assert.strictEqual(route.path, "/collections/docs/points/query");
console.log("  ✓ compileQuery");

const plan = nqql.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(plan.includes("Statement: QUERY"));
assert(plan.includes("Collection: docs"));
console.log("  ✓ explain");

assert(nqql.explainStmt(r0).includes("QUERY"));
console.log("  ✓ explainStmt");

const filtered = nqql.injectFilter(query, "tenant_id", "=", "acme");
assert(filtered.Query !== undefined);
assert.throws(
  () => nqql.injectFilter(query, "tenant_id", "contains", "acme"),
  /unsupported comparison operator/,
);
console.log("  ✓ injectFilter validation");

// Parameter binding tests
const qNamed = "QUERY 'shoes' FROM products WHERE category = :cat AND price < :max_p";
const boundNamed = nqql.bind(qNamed, { cat: "sneakers", max_p: 100 });
assert.strictEqual(
  boundNamed,
  "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND price < 100",
);
const boundNamedFn = nqql.bind(qNamed, { cat: "boots", max_p: 50 });
assert.strictEqual(
  boundNamedFn,
  "QUERY 'shoes' FROM products WHERE category = 'boots' AND price < 50",
);

const qPos = "QUERY 'shoes' FROM products WHERE category = ? AND in_stock = ?";
const boundPos = nqql.bind(qPos, ["sneakers", true]);
assert.strictEqual(
  boundPos,
  "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND in_stock = true",
);
const boundPosFn = nqql.bind(qPos, ["boots", false]);
assert.strictEqual(
  boundPosFn,
  "QUERY 'shoes' FROM products WHERE category = 'boots' AND in_stock = false",
);

assert.strictEqual(
  nqql.bind("WHERE flag = ? AND count = ?", [true, 1]),
  "WHERE flag = true AND count = 1",
);
assert.strictEqual(
  nqql.bind("WHERE $category = :cat AND $1 = 42", { cat: "boots" }),
  "WHERE $category = 'boots' AND $1 = 42",
);

assert.throws(() => nqql.bind("WHERE x = :x", [1]), /named placeholder|MIXED-STYLE/);
assert.throws(() => nqql.bind("WHERE x = ?", { x: 1 }), /positional placeholder|MIXED-STYLE/);
console.log("  ✓ parameter binding");

const stmt = nqql.parse("QUERY 'hello' FROM docs LIMIT 10")[0];
stmt.injectFilter("x", ">", 42);
assert(stmt.toJson().includes('"Gt"'));
console.log("  ✓ Stmt.injectFilter");

assert.strictEqual(r0.shardKey, null);
r0.shardKey = "europe";
assert.strictEqual(r0.shardKey, "europe");
r0.shardKey = null;
assert.strictEqual(r0.shardKey, null);
console.log("  ✓ Stmt.shardKey");

try {
  nqql.parse("invalid syntax");
  assert.fail("Should have thrown");
} catch (e) {
  assert.strictEqual(e.code, "QQL-PARSE-STATEMENT");
  assert.strictEqual(e.kind, "Parse");
  assert.deepStrictEqual(e.span, { start: 0, end: 7 });
}
console.log("  ✓ structured QQL error");

// ═══════════════════════════════════════════════════════════════════
//  2. Model selection API
// ═══════════════════════════════════════════════════════════════════

console.log("\n── 2. Model selection ──");

const models = nqql.listEmbeddingModels();
assert(Array.isArray(models) && models.length > 5, "should list multiple models");
const bge = models.find((m) => m.name === "BGESmallENV15");
assert(bge, "default BGESmallENV15 must be listed");
assert.strictEqual(bge.dim, 384);
assert(bge.modelCode.includes("bge-small"));
console.log(`  ✓ listEmbeddingModels() → ${models.length} models (BGE small = 384-d)`);

// Invalid model must fail at executor construction, not later at upsert
assert.throws(
  () => nqql.localExecutor(tmpDir("nqql-bad-model-"), { model: "not-a-real-model-xyz" }),
  /unknown embedding model/i,
);
console.log("  ✓ invalid model rejected at localExecutor()");

// Boolean legacy second arg still works
{
  const d = tmpDir("nqql-legacy-bool-");
  const e = nqql.localExecutor(d, false);
  assert(e instanceof nqql.Client);
  console.log("  ✓ localExecutor(dir, false) legacy boolean");
}

// Options object with model alias
{
  const d = tmpDir("nqql-model-alias-");
  const opts = { onDiskPayload: false, model: "bge-small-en-v1.5" };
  if (projectCache) opts.cacheDir = projectCache;
  const e = nqql.localExecutor(d, opts);
  assert(e instanceof nqql.Client);
  console.log("  ✓ localExecutor({ model: 'bge-small-en-v1.5' }) short alias");
}

// ═══════════════════════════════════════════════════════════════════
//  3. Edge executor basics
// ═══════════════════════════════════════════════════════════════════

console.log("\n── 3. Edge Executor (basic) ──");

const dataDir = tmpDir("nqql-edge-test-");
console.log("  dataDir:", dataDir);

const execOpts = { onDiskPayload: false };
if (projectCache) execOpts.cacheDir = projectCache;
const exec = nqql.localExecutor(dataDir, execOpts);
assert(exec instanceof nqql.Client);
console.log("  ✓ localExecutor() creates Client");

assert(exec.explain("QUERY 'hello' FROM docs LIMIT 10").includes("docs"));
console.log("  ✓ Client.explain");

assert(exec.explainStmt(r0).includes("QUERY"));
console.log("  ✓ Client.explainStmt");

assert.strictEqual(exec.compile("QUERY 'hello' FROM docs LIMIT 10").method, "POST");
console.log("  ✓ Client.compile");

(async () => {
  await assert.rejects(
    exec.execute("SHOW COLLECTIONS", { onError: "typo" }),
    /options\.onError must be 'stop' or 'continue'/,
  );
  console.log("  ✓ on_error validation");

  const badReport = await exec.execute("invalid syntax", { onError: "continue" });
  assert.strictEqual(badReport.ok, false);
  assert.strictEqual(badReport.failed, 1);
  assert.strictEqual(badReport.results[0].operation, "PARSE");
  console.log("  ✓ parse error → continue mode");

  // ═════════════════════════════════════════════════════════════════
  //  4. Full E2E: CREATE HYBRID → UPSERT → QUERY → COUNT → DELETE
  // ═════════════════════════════════════════════════════════════════

  console.log("\n── 4. Full E2E Pipeline ──");

  let r = await exec.execute("CREATE COLLECTION nqql_test HYBRID");
  assert.strictEqual(r.ok, true);
  console.log("  ✓ CREATE COLLECTION nqql_test HYBRID");

  const id1 = uuid(1);
  const id2 = uuid(2);
  r = await exec.execute(
    `UPSERT INTO nqql_test VALUES ` +
      `{id: "${id1}", text: "Rust is a systems programming language that runs blazingly fast", created_at: 1, meta: {cat: "tech"}}, ` +
      `{id: "${id2}", text: "Python is great for data science and machine learning", created_at: 2, meta: {cat: "tech"}}`,
  );
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ UPSERT 2 documents (text → ONNX embedding)");

  r = await exec.execute(
    "QUERY 'fast programming language' FROM nqql_test USING dense LIMIT 2",
  );
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  const qResults = r.results[0].data;
  assert(Array.isArray(qResults) && qResults.length > 0, "should return results");
  console.log(`  ✓ QUERY returned ${qResults.length} result(s) (top id: ${qResults[0]?.id})`);

  r = await exec.execute("COUNT FROM nqql_test");
  const countVal = r.results[0].data?.result?.count ?? r.results[0].data;
  assert.strictEqual(countVal, 2);
  console.log("  ✓ COUNT = 2");

  r = await exec.execute([
    "COUNT FROM nqql_test",
    "QUERY 'data science' FROM nqql_test USING dense LIMIT 1",
  ]);
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.succeeded, 2);
  console.log("  ✓ batch execute (2 statements)");

  r = await exec.execute(`DELETE FROM nqql_test WHERE id = "${id2}"`);
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ DELETE doc-2");

  r = await exec.execute("COUNT FROM nqql_test");
  const countAfterDel = r.results[0].data?.result?.count ?? r.results[0].data;
  assert.strictEqual(countAfterDel, 1);
  console.log("  ✓ COUNT after delete = 1");

  // Numeric point IDs must work (qdrant-edge NumId)
  r = await exec.execute(
    `UPSERT INTO nqql_test VALUES {id: 42, text: "numeric id works fine for edge", created_at: 3}`,
  );
  assert.strictEqual(r.ok, true, `numeric id upsert: ${JSON.stringify(r)}`);
  console.log("  ✓ UPSERT with numeric id=42");

  // Native qdrant-edge scoring/rescore variants.
  r = await exec.execute(
    "QUERY MMR TEXT 'fast programming' DIVERSITY 0.4 CANDIDATES 10 FROM nqql_test USING dense LIMIT 1",
  );
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ native MMR query");

  r = await exec.execute("QUERY SAMPLE RANDOM FROM nqql_test LIMIT 1");
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ native random sample query");

  r = await exec.execute("CREATE INDEX ON COLLECTION nqql_test FOR created_at TYPE integer");
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  r = await exec.execute("QUERY ORDER BY created_at DESC FROM nqql_test LIMIT 1");
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ native order-by query");

  r = await exec.execute(
    "WITH candidates AS (QUERY TEXT 'fast programming' USING dense LIMIT 10) " +
      "QUERY FORMULA $score * 2 DEFAULTS (score = 0.0) FROM nqql_test " +
      "PREFETCH (candidates) LIMIT 1",
  );
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ native formula rescore query");

  for (const query of [
    "QUERY CONTEXT (POSITIVE TEXT 'fast' NEGATIVE TEXT 'slow') FROM nqql_test USING dense LIMIT 1",
    "QUERY DISCOVER TARGET TEXT 'fast' CONTEXT (POSITIVE TEXT 'fast' NEGATIVE TEXT 'slow') FROM nqql_test USING dense LIMIT 1",
    "QUERY RELEVANCE FEEDBACK TARGET TEXT 'fast' FEEDBACK ((TEXT 'fast', 0.8)) STRATEGY NAIVE (a = 1, b = 1, c = 1) FROM nqql_test USING dense LIMIT 1",
  ]) {
    r = await exec.execute(query);
    assert.strictEqual(r.ok, true, JSON.stringify(r));
  }
  console.log("  ✓ native context, discover, and relevance-feedback queries");

  // ═════════════════════════════════════════════════════════════════
  //  5. Red-team: grammar / edge constraints that bite people
  // ═════════════════════════════════════════════════════════════════

  console.log("\n── 5. Red-team (edge grammar & constraints) ──");

  // 5a. Non-UUID string point IDs must FAIL loudly (not silently no-op)
  r = await exec.execute(
    `UPSERT INTO nqql_test VALUES {id: "doc-not-a-uuid", text: "should fail"}`,
    { onError: "continue" },
  );
  assertFails(r, "uuid");
  console.log("  ✓ non-UUID string id rejected (not silently dropped)");

  // 5b. DELETE of non-UUID string must also fail (was filter_map-dropped before)
  r = await exec.execute(
    `DELETE FROM nqql_test WHERE id = "doc-not-a-uuid"`,
    { onError: "continue" },
  );
  assertFails(r, "uuid");
  console.log("  ✓ DELETE non-UUID string id rejected");

  // 5c. Hybrid topology is ambiguous: callers must select dense or sparse.
  r = await exec.execute(
    "QUERY 'anything' FROM nqql_test LIMIT 1",
    { onError: "continue" },
  );
  assertFails(r, "ambiguous vector topology");
  console.log("  ✓ QUERY requires USING for ambiguous HYBRID topology");

  // 5d. Implicit embedding follows the collection's dense-only topology.
  {
    const trapDir = tmpDir("nqql-dense-trap-");
    const trap = nqql.localExecutor(trapDir, {
      onDiskPayload: false,
      ...(projectCache ? { cacheDir: projectCache } : {}),
    });
    r = await trap.execute("CREATE COLLECTION dense_only");
    assert.strictEqual(r.ok, true);
    r = await trap.execute(
      `UPSERT INTO dense_only VALUES {id: "${uuid(9)}", text: "no sparse vectors here"}`,
      { onError: "continue" },
    );
    assert.strictEqual(r.ok, true, JSON.stringify(r));
    r = await trap.execute("QUERY 'sparse' FROM dense_only LIMIT 1");
    assert.strictEqual(r.ok, true, JSON.stringify(r));
    console.log("  ✓ dense-only collection receives only dense implicit embeddings");
    await trap.close();
    try {
      fs.rmSync(trapDir, { recursive: true, force: true });
    } catch (_) {}
  }

  // 5e. SELECT is rejected — not a QQL keyword
  assert(!nqql.isValid("SELECT * FROM docs"));
  console.log("  ✓ SELECT rejected (use QUERY)");

  // 5f. SHARD routing is parseable but unsupported at edge execute
  assert(nqql.isValid(`QUERY 'x' FROM nqql_test USING dense SHARD 'tenant-a' LIMIT 1`));
  r = await exec.execute(
    `QUERY 'x' FROM nqql_test USING dense SHARD 'tenant-a' LIMIT 1`,
    { onError: "continue" },
  );
  assert.strictEqual(r.ok, false, "edge must reject clustered SHARD routing");
  console.log("  ✓ SHARD clause is rejected explicitly in edge mode");

  // 5g. GROUP BY not supported in edge
  r = await exec.execute(
    `QUERY 'x' FROM nqql_test USING dense GROUP BY meta.cat LIMIT 5`,
    { onError: "continue" },
  );
  assert.strictEqual(r.ok, false, "query_groups should fail in edge");
  console.log("  ✓ GROUP BY rejected in edge mode");

  // The original qdrant-edge API accepts vectors for recommendations, while
  // QQL's public RECOMMEND syntax supplies point references. Do not add a
  // client-side point-to-vector lookup here; reject it explicitly.
  r = await exec.execute(
    "QUERY RECOMMEND POSITIVE (42) STRATEGY best_score FROM nqql_test USING dense LIMIT 1",
    { onError: "continue" },
  );
  assertFails(r, "point-id");
  console.log("  ✓ point-reference recommendation rejected explicitly");

  // 5h. Mismatched USING MODEL on a locked local embedder
  r = await exec.execute(
    `QUERY 'x' FROM nqql_test USING dense MODEL 'definitely-not-loaded' LIMIT 1`,
    { onError: "continue" },
  );
  assert.strictEqual(r.ok, false, "wrong MODEL must fail");
  console.log("  ✓ USING MODEL mismatch rejected (embedder is locked at construction)");

  // 5i. Empty / garbage scripts
  r = await exec.execute("", { onError: "continue" });
  // empty may parse to 0 stmts
  console.log(`  ✓ empty script → ok=${r.ok} succeeded=${r.succeeded}`);

  // 5j. CREATE COLLECTION with explicit wrong dimension vs model
  {
    const dimDir = tmpDir("nqql-dim-mismatch-");
    const dimExec = nqql.localExecutor(dimDir, {
      onDiskPayload: false,
      ...(projectCache ? { cacheDir: projectCache } : {}),
    });
    r = await dimExec.execute(
      "CREATE COLLECTION wrong_dim (dense VECTOR(16, COSINE), sparse SPARSE)",
    );
    assert.strictEqual(r.ok, true);
    r = await dimExec.execute(
      `UPSERT INTO wrong_dim VALUES {id: "${uuid(3)}", text: "dim mismatch"}`,
      { onError: "continue" },
    );
    assert.strictEqual(
      r.ok,
      false,
      "16-d collection + 384-d embedder must fail on upsert",
    );
    console.log("  ✓ dimension mismatch (VECTOR(16) vs 384-d model) fails on UPSERT");
    await dimExec.close();
    try {
      fs.rmSync(dimDir, { recursive: true, force: true });
    } catch (_) {}
  }

  // 5k. SHOW COLLECTIONS works
  r = await exec.execute("SHOW COLLECTIONS");
  assert.strictEqual(r.ok, true, JSON.stringify(r));
  console.log("  ✓ SHOW COLLECTIONS");

  // ═════════════════════════════════════════════════════════════════
  //  6. Standalone execute / executeStmt
  // ═════════════════════════════════════════════════════════════════

  console.log("\n── 6. Standalone paths ──");

  const standaloneDir = tmpDir("nqql-edge-standalone-");
  const standaloneReport = await nqql.execute("CREATE COLLECTION standalone_test HYBRID", {
    dataDir: standaloneDir,
    onDiskPayload: false,
    ...(projectCache ? { cacheDir: projectCache } : {}),
  });
  assert.strictEqual(standaloneReport.ok, true);
  console.log("  ✓ standalone execute() creates collection");

  const countStmt = nqql.parse("COUNT FROM standalone_test")[0];
  const stmtReport = await nqql.executeStmt(countStmt, {
    dataDir: standaloneDir,
    onDiskPayload: false,
    ...(projectCache ? { cacheDir: projectCache } : {}),
  });
  assert.strictEqual(stmtReport.ok, true);
  fs.rmSync(standaloneDir, { recursive: true, force: true });
  console.log("  ✓ executeStmt()");

  // ═════════════════════════════════════════════════════════════════
  //  Cleanup
  // ═════════════════════════════════════════════════════════════════

  console.log("\n── Done ──");
  try {
    await exec.execute("DROP COLLECTION nqql_test");
  } catch (_) {}
  await exec.close();
  fs.rmSync(dataDir, { recursive: true, force: true });

  console.log("\n✅ All tests passed!");
})().catch((e) => {
  console.error("\n❌ FAILED:", e && e.stack ? e.stack : e);
  try {
    fs.rmSync(dataDir, { recursive: true, force: true });
  } catch (_) {}
  process.exit(1);
});
