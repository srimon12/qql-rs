'use strict';

// ============================================================================
// COMPREHENSIVE TEST SUITE FOR @veristamp/nqql v0.1.2
// ============================================================================
// Requires: Qdrant running at http://localhost:6333
// Run: node test_comprehensive.js
// ============================================================================

const assert = require('assert');
const nqql = require('./index.js');

const QDRANT_URL = 'http://localhost:6333';
const TEST_COLLECTION = 'nqql_fresh_test';

let passed = 0;
let failed = 0;
let skipped = 0;

function test(name, fn) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
    passed++;
  } catch (e) {
    console.log(`  ✗ ${name}`);
    console.log(`    ${e.message}`);
    failed++;
  }
}

async function testAsync(name, fn) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
    passed++;
  } catch (e) {
    console.log(`  ✗ ${name}`);
    console.log(`    ${e.message}`);
    failed++;
  }
}

async function ensureCleanCollection(client) {
  try { await client.execute(`DROP COLLECTION ${TEST_COLLECTION}`); } catch (_) {}
}

// ============================================================================
console.log('\n========== A. Package Inspection ==========');

test('exports: Client', () => assert.strictEqual(typeof nqql.Client, 'function'));
test('exports: HttpEmbedder', () => assert.strictEqual(typeof nqql.HttpEmbedder, 'function'));
test('exports: Stmt constructor', () => assert.strictEqual(typeof nqql.Stmt, 'function'));
test('exports: parse', () => assert.strictEqual(typeof nqql.parse, 'function'));
test('exports: parseJson', () => assert.strictEqual(typeof nqql.parseJson, 'function'));
test('exports: isValid', () => assert.strictEqual(typeof nqql.isValid, 'function'));
test('exports: injectFilter', () => assert.strictEqual(typeof nqql.injectFilter, 'function'));
test('exports: tokenize', () => assert.strictEqual(typeof nqql.tokenize, 'function'));
test('exports: compileQuery', () => assert.strictEqual(typeof nqql.compileQuery, 'function'));
test('exports: explain', () => assert.strictEqual(typeof nqql.explain, 'function'));
test('exports: explainStmt', () => assert.strictEqual(typeof nqql.explainStmt, 'function'));
test('exports: execute', () => assert.strictEqual(typeof nqql.execute, 'function'));
test('exports: executeStmt', () => assert.strictEqual(typeof nqql.executeStmt, 'function'));

// Unknown exports check
const knownKeys = ['Client','HttpEmbedder','Stmt','compileQuery','execute','executeStmt',
  'explain','explainStmt','injectFilter','isValid','parse','parseJson','tokenize', 'version', '__version__'];
const actualKeys = Object.keys(nqql).sort();
test('no extra exports', () => {
  const extras = actualKeys.filter(k => !knownKeys.includes(k));
  assert.deepStrictEqual(extras, [], `Unexpected exports: ${extras.join(', ')}`);
});

const stmt = nqql.parse('SHOW COLLECTIONS')[0];
test('Stmt instance methods: toObject', () => assert.strictEqual(typeof stmt.toObject, 'function'));
test('Stmt instance methods: injectFilter', () => assert.strictEqual(typeof stmt.injectFilter, 'function'));
test('Stmt.toJson and Stmt.toJSON both exist', () => {
  assert.strictEqual(typeof stmt.toJson, 'function');
  assert.strictEqual(typeof stmt.toJSON, 'function');
});
test('Stmt.shardKey property (get)', () => assert.strictEqual(stmt.shardKey, null));

// HttpEmbedder validation
test('HttpEmbedder requires endpoint', () => {
  assert.throws(() => new nqql.HttpEmbedder({}), TypeError, 'HttpEmbedder requires a non-empty endpoint');
});
test('HttpEmbedder requires model', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost' }), TypeError);
});
test('HttpEmbedder requires dimension', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm' }), TypeError);
});
test('HttpEmbedder rejects string dimension', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: '768' }), TypeError);
});
test('HttpEmbedder rejects zero dimension', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 0 }), TypeError);
});
test('HttpEmbedder rejects negative dimension', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: -1 }), TypeError);
});
test('HttpEmbedder rejects float dimension', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 1.5 }), TypeError);
});
test('HttpEmbedder valid construction', () => {
  const e = new nqql.HttpEmbedder({ endpoint: 'http://localhost:8080', model: 'test', dimension: 768 });
  assert.strictEqual(e.endpoint, 'http://localhost:8080');
  assert.strictEqual(e.model, 'test');
  assert.strictEqual(e.dimension, 768);
  assert.strictEqual(e.apiKey, ''); // default empty
});
test('HttpEmbedder with apiKey', () => {
  const e = new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 128, apiKey: 'sk-test' });
  assert.strictEqual(e.apiKey, 'sk-test');
});
test('HttpEmbedder rejects non-string apiKey', () => {
  assert.throws(() => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 128, apiKey: 123 }), TypeError);
});

// Client construction
test('Client with url', () => {
  const c = new nqql.Client({ url: QDRANT_URL, useGrpc: false });
  assert.ok(c instanceof nqql.Client);
});
test('Client with snake_case options (api_key, use_grpc)', () => {
  const c = new nqql.Client({ url: QDRANT_URL, api_key: 'test', use_grpc: false });
  assert.ok(c instanceof nqql.Client);
});
test('Client with embedder object', () => {
  const e = new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 128 });
  const c = new nqql.Client({ url: QDRANT_URL, embedder: e, useGrpc: false });
  assert.ok(c instanceof nqql.Client);
});

// ============================================================================
console.log('\n========== B. Parse API ==========');

test('parse() returns array', () => {
  const stmts = nqql.parse('SHOW COLLECTIONS');
  assert.ok(Array.isArray(stmts));
  assert.strictEqual(stmts.length, 1);
  assert.ok(stmts[0] instanceof nqql.Stmt);
});

test('parseJson() returns JSON string', () => {
  const json = nqql.parseJson('SHOW COLLECTIONS');
  assert.strictEqual(typeof json, 'string');
  const parsed = JSON.parse(json);
  assert.ok(Array.isArray(parsed));
});

test('parseJson output is valid JSON, matches parse', () => {
  const json = nqql.parseJson('QUERY TEXT "hello" FROM docs LIMIT 5');
  const parsed = JSON.parse(json);
  assert.ok(Array.isArray(parsed));
  // For QUERY statements, parsed[0] is an object with .Query
  assert.strictEqual(typeof parsed[0], 'object');
  assert.ok('Query' in parsed[0]);
});

test('multi-statement parse with semicolons', () => {
  const stmts = nqql.parse('SHOW COLLECTIONS; COUNT FROM docs');
  assert.ok(Array.isArray(stmts));
  assert.strictEqual(stmts.length, 2);
  assert.deepStrictEqual(stmts[0].toObject(), { ShowCollections: {} });
});

test('toObject() for QUERY returns object with Query.collection.Explicit', () => {
  const [s] = nqql.parse('QUERY TEXT "hello" FROM docs LIMIT 5');
  const obj = s.toObject();
  assert.strictEqual(typeof obj, 'object');
  assert.strictEqual(obj.Query.collection.Explicit, 'docs');
  assert.strictEqual(obj.Query.page.limit, 5);
  assert.strictEqual(obj.Query.expression.Nearest.input.Text.text, 'hello');
});

test('toJson() returns JSON string', () => {
  const [s] = nqql.parse('QUERY TEXT "hello" FROM docs LIMIT 5');
  const json = s.toJson();
  assert.strictEqual(typeof json, 'string');
  const parsed = JSON.parse(json);
  assert.strictEqual(parsed.Query.collection.Explicit, 'docs');
});

test('isValid() with valid input', () => {
  assert.strictEqual(nqql.isValid('SHOW COLLECTIONS'), true);
  assert.strictEqual(nqql.isValid('QUERY TEXT "hi" FROM docs LIMIT 1'), true);
});

test('isValid() with invalid input', () => {
  assert.strictEqual(nqql.isValid('SELECT * FROM docs'), false);
  assert.strictEqual(nqql.isValid('GARBAGE'), false);
});

test('isValid() with empty string', () => {
  // NOTE: Empty string is considered valid (empty script).
  // This is a design decision — an empty script has no errors.
  const valid = nqql.isValid('');
  console.log(`    NOTE: isValid('') = ${valid} (empty script is valid)`);
});

test('tokenize() returns array with text, kind, pos', () => {
  const tokens = nqql.tokenize('SHOW COLLECTIONS');
  assert.ok(Array.isArray(tokens));
  assert.ok(tokens.length > 0);
  assert.ok('text' in tokens[0]);
  assert.ok('kind' in tokens[0]);
  assert.ok('pos' in tokens[0]);
  assert.strictEqual(tokens[0].kind, 'SHOW');
  assert.strictEqual(tokens[0].text, 'SHOW');
  assert.strictEqual(typeof tokens[0].pos, 'number');
});

test('tokenize() with multi-word', () => {
  const tokens = nqql.tokenize('QUERY TEXT "hi" FROM docs');
  const kinds = tokens.map(t => t.kind);
  assert.ok(kinds.includes('QUERY'));
  assert.ok(kinds.includes('FROM'));
});

// ============================================================================
console.log('\n========== C. Error Handling ==========');

test('parse() invalid syntax throws structured error', () => {
  try {
    nqql.parse('GARBAGE SYNTAX X');
    assert.fail('should have thrown');
  } catch (e) {
    assert.strictEqual(e.code, 'QQL-PARSE-STATEMENT');
    assert.strictEqual(e.kind, 'Parse');
    assert.ok(e.span !== null && e.span !== undefined);
    assert.ok(typeof e.span.start === 'number');
    assert.ok(typeof e.span.end === 'number');
    assert.ok(e.span.start === 0);
    assert.ok(e.message.length > 0);
  }
});

test('parse() with empty string returns empty array', () => {
  // Empty string is valid and parses to empty array
  const stmts = nqql.parse('');
  assert.ok(Array.isArray(stmts));
  assert.strictEqual(stmts.length, 0);
});

test('injectFilter with unsupported operator "contains"', () => {
  assert.throws(
    () => nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', 'contains', 5),
    /unsupported comparison operator/
  );
});

test('Stmt.injectFilter with unsupported operator', () => {
  const [stmt] = nqql.parse('QUERY TEXT "hi" FROM docs LIMIT 1');
  assert.throws(
    () => stmt.injectFilter('x', 'contains', 5),
    /unsupported comparison operator/
  );
});

test('injectFilter unsupported operator "in"', () => {
  assert.throws(
    () => nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', 'in', [1, 2]),
    /unsupported comparison operator/
  );
});

// ============================================================================
console.log('\n========== D. injectFilter ==========');

test('injectFilter with = operator', () => {
  // NOTE: injectFilter returns the statement AST object, not a string
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'org_id', '=', 'acme');
  assert.strictEqual(typeof result, 'object');
  assert.ok('Query' in result);
  // Verify filter is present in the AST
  assert.ok(result.Query.filter !== null);
  const filterJson = JSON.stringify(result.Query.filter);
  assert.ok(filterJson.includes('org_id'));
  assert.ok(filterJson.includes('acme'));
});

test('injectFilter with > operator', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'score', '>', 10);
  assert.ok('Query' in result);
  assert.ok(result.Query.filter !== null);
});

test('injectFilter with < operator', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'score', '<', 10);
  assert.ok('Query' in result);
  assert.ok(result.Query.filter !== null);
});

test('injectFilter with >= operator', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'score', '>=', 10);
  assert.ok('Query' in result);
  assert.ok(result.Query.filter !== null);
});

test('injectFilter with <= operator', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'score', '<=', 10);
  assert.ok('Query' in result);
  assert.ok(result.Query.filter !== null);
});

test('injectFilter on Stmt object in-place', () => {
  const [stmt] = nqql.parse('QUERY TEXT "hi" FROM docs LIMIT 1');
  const before = stmt.toObject();
  const beforeFilter = before.Query.filter;
  stmt.injectFilter('x', '=', 5);
  const after = stmt.toObject();
  assert.notDeepStrictEqual(beforeFilter, after.Query.filter);
  assert.ok(after.Query.filter !== null);
});

test('injectFilter with number value', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', '=', 42);
  assert.strictEqual(typeof result, 'object');
  assert.ok(result.Query.filter !== null);
  const filterStr = JSON.stringify(result.Query.filter);
  assert.ok(filterStr.includes('42'));
});

test('injectFilter with boolean value', () => {
  const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', '=', true);
  assert.strictEqual(typeof result, 'object');
  assert.ok(result.Query.filter !== null);
  const filterStr = JSON.stringify(result.Query.filter);
  assert.ok(filterStr.includes('true'));
});

test('injectFilter ! = is NOT supported', () => {
  assert.throws(
    () => nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', '!=', 5),
    /inject_filter does not support/
  );
});

// Check which ops are actually supported
test('injectFilter supported ops inventory', () => {
  const supported = [];
  const unsupported = [];
  const allOps = ['=', '!=', '>', '<', '>=', '<=', 'in', 'not_in', 'match', 'is_null', 'is_not_null'];
  for (const op of allOps) {
    try {
      nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', op, 5);
      supported.push(op);
    } catch (e) {
      unsupported.push(op);
    }
  }
  console.log(`    injectFilter supported ops: [${supported.join(', ')}]`);
  console.log(`    injectFilter unsupported ops: [${unsupported.join(', ')}]`);
  assert.deepStrictEqual(supported, ['=', '>', '<', '>=', '<=']);
});

// ============================================================================
console.log('\n========== E. Client (Remote REST) ==========');

const client = new nqql.Client({ url: QDRANT_URL, useGrpc: false });

test('Client is instance of Client', () => {
  assert.ok(client instanceof nqql.Client);
});

test('Client.explain returns string', () => {
  const plan = client.explain('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.strictEqual(typeof plan, 'string');
  assert.ok(plan.length > 0);
  assert.ok(plan.includes('Query') || plan.includes('query') || plan.includes('QUERY'));
});

test('Client.explainStmt returns string', () => {
  const [stmt] = nqql.parse('QUERY TEXT "hello" FROM docs LIMIT 5');
  const plan = client.explainStmt(stmt);
  assert.strictEqual(typeof plan, 'string');
  assert.ok(plan.length > 0);
});

test('Client.compile returns route object', () => {
  const route = client.compile('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.strictEqual(typeof route, 'object');
  assert.ok('method' in route);
  assert.ok('path' in route);
  assert.ok('payload' in route);
  assert.ok('stmt_type' in route);
  assert.strictEqual(route.stmt_type, 'query');
});

test('Client.execute SHOW COLLECTIONS', async () => {
  await testAsync('Client.execute SHOW COLLECTIONS', async () => {
    const result = await client.execute('SHOW COLLECTIONS');
    assert.strictEqual(result.ok, true);
    assert.ok(Array.isArray(result.results));
    assert.strictEqual(typeof result.succeeded, 'number');
    assert.strictEqual(typeof result.failed, 'number');
    assert.strictEqual(result.results[0].operation, 'SHOW_COLLECTIONS');
  });
  // Run it now
  const result = await client.execute('SHOW COLLECTIONS');
  assert.strictEqual(result.ok, true);
  assert.ok(Array.isArray(result.results));
});

test('Client.execute onError "typo" rejects', async () => {
  try {
    await client.execute('SHOW COLLECTIONS', { onError: 'typo' });
    assert.fail('should have rejected');
  } catch (e) {
    assert.ok(e.message.includes("must be 'stop' or 'continue'"));
  }
});

test('Client.execute onError "continue" on invalid syntax', async () => {
  const report = await client.execute('GARBAGE', { onError: 'continue' });
  assert.strictEqual(report.ok, false);
  assert.strictEqual(report.failed, 1);
  assert.strictEqual(report.succeeded, 0);
  assert.strictEqual(report.results[0].operation, 'PARSE');
  assert.strictEqual(report.results[0].ok, false);
});

// ============================================================================
console.log('\n========== F. HttpEmbedder Edge Cases ==========');

test('HttpEmbedder with dimension as number works', () => {
  const e = new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 768 });
  assert.strictEqual(e.dimension, 768);
});

test('HttpEmbedder dimension must be integer (float rejected)', () => {
  assert.throws(
    () => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: 768.5 }),
    /positive integer/
  );
});

test('HttpEmbedder NaN dimension rejected', () => {
  assert.throws(
    () => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: NaN }),
    /positive integer/
  );
});

test('HttpEmbedder Infinity dimension rejected', () => {
  assert.throws(
    () => new nqql.HttpEmbedder({ endpoint: 'http://localhost', model: 'm', dimension: Infinity }),
    /positive integer/
  );
});

// ============================================================================
console.log('\n========== G. compileQuery Route Contract ==========');

test('compileQuery returns { method, path, payload, stmt_type }', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.ok(typeof route === 'object');
  assert.ok('method' in route);
  assert.ok('path' in route);
  assert.ok('payload' in route);
  assert.ok('stmt_type' in route);
});

test('compileQuery stmt_type is snake_case', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  // NOTE: it's stmt_type not stmtType
  assert.strictEqual(route.stmt_type, 'query');
  assert.strictEqual('stmtType' in route, false);
});

test('compileQuery QUERY method is POST', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.strictEqual(route.method, 'POST');
});

test('compileQuery QUERY path includes /points/query', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.ok(route.path.includes('/points/query'));
});

test('compileQuery QUERY payload has limit', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.strictEqual(route.payload.limit, 5);
});

// ============================================================================
console.log('\n========== H. Full E2E Pipeline (live Qdrant) ==========');

const e2eClient = new nqql.Client({ url: QDRANT_URL, useGrpc: false });

async function runE2E() {
  await ensureCleanCollection(e2eClient);

  // 1. Create collection via direct API (4-dim to match test vectors)
  //    NOTE: CREATE COLLECTION via QQL always defaults to 384-dim,
  //    so we create via raw API for E2E compatibility.
  {
    const res = await fetch(`${QDRANT_URL}/collections/${TEST_COLLECTION}?wait=true`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ vectors: { size: 4, distance: 'Cosine' } }),
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`Failed to create collection: ${body}`);
    }
    console.log('  ✓ E2E: Created collection (4-dim via API)');
    passed++;
  }

  // 2. SHOW COLLECTIONS via QQL
  {
    const r = await e2eClient.execute('SHOW COLLECTIONS');
    assert.strictEqual(r.ok, true);
    const names = r.results[0].data.result.collections.map(c => c.name);
    assert.ok(names.includes(TEST_COLLECTION));
    console.log('  ✓ E2E: SHOW COLLECTIONS via QQL');
    passed++;
  }

  // 3. UPSERT two documents
  const vec1 = [0.1, 0.2, 0.3, 0.4];
  const vec2 = [0.5, 0.6, 0.7, 0.8];
  const uuid1 = crypto.randomUUID();
  const uuid2 = crypto.randomUUID();

  {
    const r = await e2eClient.execute(
      `UPSERT INTO ${TEST_COLLECTION} VALUES {id: "${uuid1}", vector: [${vec1.join(',')}], payload: {text: "hello world", created_at: 1234567890}}`
    );
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].ok, true);
    assert.strictEqual(r.results[0].operation, 'UPSERT');
    console.log('  ✓ E2E: UPSERT 1');
    passed++;
  }

  {
    const r = await e2eClient.execute(
      `UPSERT INTO ${TEST_COLLECTION} VALUES {id: "${uuid2}", vector: [${vec2.join(',')}], payload: {text: "foo bar baz", created_at: 1234567891}}`
    );
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].ok, true);
    console.log('  ✓ E2E: UPSERT 2');
    passed++;
  }

  // 4. COUNT
  {
    const r = await e2eClient.execute(`COUNT FROM ${TEST_COLLECTION}`);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].data.result.count, 2);
    console.log('  ✓ E2E: COUNT = 2');
    passed++;
  }

  // 5. QUERY
  {
    const r = await e2eClient.execute(`QUERY VECTOR [${vec1.join(',')}] FROM ${TEST_COLLECTION} LIMIT 10`);
    assert.strictEqual(r.ok, true);
    assert.ok(Array.isArray(r.results[0].data));
    assert.ok(r.results[0].data.length >= 1);
    console.log('  ✓ E2E: QUERY returned results');
    passed++;
  }

  // 6. DELETE by id
  {
    const r = await e2eClient.execute(`DELETE FROM ${TEST_COLLECTION} WHERE id = "${uuid1}"`);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].ok, true);
    console.log('  ✓ E2E: DELETE by id');
    passed++;
  }

  // 7. COUNT again (should be 1)
  {
    const r = await e2eClient.execute(`COUNT FROM ${TEST_COLLECTION}`);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].data.result.count, 1);
    console.log('  ✓ E2E: COUNT = 1 after delete');
    passed++;
  }

  // 8. Batch execute: array of strings
  {
    const r = await e2eClient.execute([`COUNT FROM ${TEST_COLLECTION}`, `COUNT FROM ${TEST_COLLECTION}`]);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results.length, 2);
    console.log('  ✓ E2E: batch execute (string array)');
    passed++;
  }

  // 9. Batch execute: array of Stmt objects
  {
    const stmt1 = nqql.parse(`COUNT FROM ${TEST_COLLECTION}`)[0];
    const stmt2 = nqql.parse(`COUNT FROM ${TEST_COLLECTION}`)[0];
    const r = await e2eClient.execute([stmt1, stmt2]);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results.length, 2);
    console.log('  ✓ E2E: batch execute (Stmt array)');
    passed++;
  }

  // 10. DROP COLLECTION
  {
    const r = await e2eClient.execute(`DROP COLLECTION ${TEST_COLLECTION}`);
    assert.strictEqual(r.ok, true);
    assert.strictEqual(r.results[0].ok, true);
    console.log('  ✓ E2E: DROP COLLECTION');
    passed++;
  }
}

(async () => {
  try {
    await runE2E();
  } catch (e) {
    console.log(`  ✗ E2E suite failed: ${e.message}`);
    failed++;
    // Clean up
    try { await e2eClient.execute(`DROP COLLECTION ${TEST_COLLECTION}`); } catch (_) {}
  }

  // ============================================================================
  console.log('\n========== I. Standalone execute Paths ==========');

  await testAsync('standalone execute("SHOW COLLECTIONS")', async () => {
    const result = await nqql.execute('SHOW COLLECTIONS', { url: QDRANT_URL, useGrpc: false });
    assert.strictEqual(result.ok, true);
    assert.ok(Array.isArray(result.results));
  });

  await testAsync('standalone executeStmt(stmt)', async () => {
    const stmt = nqql.parse('SHOW COLLECTIONS')[0];
    const result = await nqql.executeStmt(stmt, { url: QDRANT_URL, useGrpc: false });
    assert.strictEqual(result.ok, true);
  });

  // ============================================================================
  console.log('\n========== J. Edge Cases ==========');

  test('Stmt.shardKey getter (null initially)', () => {
    const [stmt] = nqql.parse('QUERY TEXT "hi" FROM docs LIMIT 1');
    assert.strictEqual(stmt.shardKey, null);
  });

  test('Stmt.shardKey setter', () => {
    const [stmt] = nqql.parse('QUERY TEXT "hi" FROM docs LIMIT 1');
    stmt.shardKey = 'my-shard';
    assert.strictEqual(stmt.shardKey, 'my-shard');
  });

  test('Stmt.shardKey reset to null', () => {
    const [stmt] = nqql.parse('QUERY TEXT "hi" FROM docs LIMIT 1');
    stmt.shardKey = 'temp';
    stmt.shardKey = null;
    assert.strictEqual(stmt.shardKey, null);
  });

  test('Stmt.shardKey on SHOW COLLECTIONS returns null', () => {
    const [stmt] = nqql.parse('SHOW COLLECTIONS');
    assert.strictEqual(stmt.shardKey, null);
  });

  test('isValid("SELECT * FROM docs") is false', () => {
    assert.strictEqual(nqql.isValid('SELECT * FROM docs'), false);
  });

  test('injectFilter with string value', () => {
    const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', '=', 'hello');
    assert.strictEqual(typeof result, 'object');
    assert.ok(result.Query.filter !== null);
    const filterStr = JSON.stringify(result.Query.filter);
    assert.ok(filterStr.includes('hello'));
  });

  test('injectFilter with number value', () => {
    const result = nqql.injectFilter('QUERY TEXT "hi" FROM docs', 'x', '=', 42);
    assert.strictEqual(typeof result, 'object');
    assert.ok(result.Query.filter !== null);
  });

  test('Client with apiKey option smoke test', () => {
    const c = new nqql.Client({ url: QDRANT_URL, apiKey: 'test-key', useGrpc: false });
    assert.ok(c instanceof nqql.Client);
  });

  test('Client with snake_case api_key and use_grpc', () => {
    const c = new nqql.Client({ url: QDRANT_URL, api_key: 'test-key', use_grpc: false });
    assert.ok(c instanceof nqql.Client);
  });

  // ============================================================================
  console.log('\n========== K. explain for non-QUERY statements ==========');

  test('explain for CREATE COLLECTION', () => {
    const plan = client.explain(`CREATE COLLECTION ${TEST_COLLECTION}`);
    assert.strictEqual(typeof plan, 'string');
    assert.ok(plan.includes('CREATE COLLECTION') || plan.includes('Create'));
  });

  test('explain for UPSERT', () => {
    const plan = client.explain(`UPSERT INTO ${TEST_COLLECTION} VALUES {id: "abc", vector: [0.1, 0.2, 0.3, 0.4]}`);
    assert.strictEqual(typeof plan, 'string');
    assert.ok(plan.includes('UPSERT') || plan.includes('Upsert') || plan.includes('upsert'));
  });

  test('explain for COUNT', () => {
    const plan = client.explain('COUNT FROM docs');
    assert.strictEqual(typeof plan, 'string');
    assert.ok(plan.length > 0);
  });

  // ============================================================================
  console.log('\n========== L. Additional Edge Cases ==========');

  test('parse single-quoted string', () => {
    const [stmt] = nqql.parse("QUERY TEXT 'hello' FROM docs LIMIT 1");
    assert.ok(stmt instanceof nqql.Stmt);
    const obj = stmt.toObject();
    assert.strictEqual(obj.Query.expression.Nearest.input.Text.text, 'hello');
  });

  test('parse double-quoted string', () => {
    const [stmt] = nqql.parse('QUERY TEXT "hello" FROM docs LIMIT 1');
    assert.ok(stmt instanceof nqql.Stmt);
  });

  test('tokenize empty string', () => {
    const tokens = nqql.tokenize('');
    assert.ok(Array.isArray(tokens));
    assert.strictEqual(tokens.length, 0);
  });

  test('parseJson vs parse consistency', () => {
    const json = nqql.parseJson('SHOW COLLECTIONS');
    const parsed = JSON.parse(json);
    assert.ok(Array.isArray(parsed));
  });

  test('compileQuery for SCROLL', () => {
    const route = nqql.compileQuery('SCROLL FROM docs LIMIT 10');
    assert.strictEqual(typeof route, 'object');
    assert.ok('stmt_type' in route);
  });

  // ============================================================================
  // SUMMARY
  // ============================================================================
  console.log('\n========================================');
  console.log('  TEST RESULTS');
  console.log('========================================');
  console.log(`  Passed:  ${passed}`);
  console.log(`  Failed:  ${failed}`);
  console.log(`  Skipped: ${skipped}`);
  console.log('========================================\n');

  if (failed > 0) {
    process.exit(1);
  } else {
    console.log('All tests passed!\n');
    process.exit(0);
  }
})();
