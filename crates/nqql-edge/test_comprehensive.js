'use strict';

// ============================================================================
// COMPREHENSIVE TEST SUITE FOR @veristamp/nqql-edge
// ============================================================================

const assert = require('assert');
const http = require('http');
const path = require('path');
const os = require('os');
const fs = require('fs');
const nqql = require('./index.js');

const TEST_DIR = path.join(os.tmpdir(), 'nqql_edge_comprehensive_' + Date.now());

let passed = 0;
let failed = 0;

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

// ============================================================================
console.log('\n========== A. Package Inspection ==========');

test('exports: Client', () => assert.strictEqual(typeof nqql.Client, 'function'));
test('exports: Stmt constructor', () => assert.strictEqual(typeof nqql.Stmt, 'function'));
test('exports: localExecutor', () => assert.strictEqual(typeof nqql.localExecutor, 'function'));
test('exports: listEmbeddingModels', () => assert.strictEqual(typeof nqql.listEmbeddingModels, 'function'));
test('exports: httpExecutor', () => assert.strictEqual(typeof nqql.httpExecutor, 'function'));
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
test('exports: version', () => assert.strictEqual(typeof nqql.version, 'string'));

const knownKeys = ['Client','Stmt','compileQuery','execute','executeStmt',
  'explain','explainStmt','httpExecutor','injectFilter','isValid','listEmbeddingModels',
  'localExecutor','parse','parseJson','tokenize', 'version', '__version__'];
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
  assert.strictEqual(stmt.toJson(), stmt.toJSON());
});
test('Stmt.shardKey property (get)', () => assert.strictEqual(stmt.shardKey, null));
test('Stmt.shardKey setter supports DELETE PAYLOAD', () => {
  const [stmt] = nqql.parse("DELETE PAYLOAD draft FROM docs WHERE status = 'archived'");
  stmt.shardKey = 'tenant-a';
  assert.strictEqual(stmt.shardKey, 'tenant-a');
  assert.strictEqual(stmt.toObject().DeletePayload.shard_key, 'tenant-a');
});
test('Stmt.shardKey from SHARD clause and property', () => {
  const [s] = nqql.parse("DELETE PAYLOAD draft FROM docs WHERE status = 'x' SHARD 'tenant'");
  assert.strictEqual(s.shardKey, 'tenant');
  const [s2] = nqql.parse("QUERY TEXT 'x' FROM docs LIMIT 1");
  s2.shardKey = 'acme';
  assert.strictEqual(s2.shardKey, 'acme');
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

test('toObject() for COUNT uses { Explicit: "docs" } for collection', () => {
  const [s] = nqql.parse('COUNT FROM docs');
  const obj = s.toObject();
  assert.strictEqual(typeof obj, 'object');
  assert.strictEqual(obj.Count.collection.Explicit, 'docs');
});

test('toObject() for SHOW COLLECTIONS returns object { ShowCollections: {} }', () => {
  const [s] = nqql.parse('SHOW COLLECTIONS');
  const obj = s.toObject();
  assert.strictEqual(typeof obj, 'object');
  assert.deepStrictEqual(obj, { ShowCollections: {} });
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
  assert.strictEqual(nqql.isValid(''), true);
});

test('tokenize() returns array with text, kind, pos', () => {
  const tokens = nqql.tokenize('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.ok(Array.isArray(tokens));
  assert.ok(tokens.length > 0);
  assert.strictEqual(typeof tokens[0].kind, 'string');
  assert.strictEqual(typeof tokens[0].text, 'string');
  assert.strictEqual(typeof tokens[0].pos, 'number');
});

// ============================================================================
console.log('\n========== C. Error Handling ==========');

test('parse() invalid syntax throws structured error', () => {
  assert.throws(() => nqql.parse('INVALID QUERY'), (err) => {
    return err.code === 'QQL-PARSE-STATEMENT' && err.kind === 'Parse';
  });
});

test('parse() with empty string returns empty array', () => {
  const stmts = nqql.parse('');
  assert.deepStrictEqual(stmts, []);
});

test('injectFilter with unsupported operator "contains"', () => {
  assert.throws(
    () => nqql.injectFilter('QUERY TEXT "x" FROM docs', 'tenant_id', 'contains', 'acme'),
    /unsupported comparison operator/,
  );
});

test('Stmt.injectFilter with unsupported operator', () => {
  const [s] = nqql.parse('QUERY TEXT "x" FROM docs');
  assert.throws(
    () => s.injectFilter('tenant_id', 'contains', 'acme'),
    /unsupported comparison operator/,
  );
});

// ============================================================================
console.log('\n========== D. filter injection ==========');

test('injectFilter with = operator', () => {
  const ast = nqql.injectFilter('QUERY TEXT "x" FROM docs', 'tenant_id', '=', 'acme');
  assert.strictEqual(typeof ast, 'object');
});

test('injectFilter with > operator', () => {
  const ast = nqql.injectFilter('QUERY TEXT "x" FROM docs', 'age', '>', 18);
  assert.strictEqual(typeof ast, 'object');
});

test('injectFilter Stmt method in-place', () => {
  const [s] = nqql.parse('QUERY TEXT "x" FROM docs');
  s.injectFilter('tenant_id', '=', 'acme');
  const obj = s.toObject();
  assert.ok(obj.Query.filter !== null);
});

// ============================================================================
console.log('\n========== E. Route Compilation & Model Discovery ==========');

test('compileQuery returns route object', () => {
  const route = nqql.compileQuery('QUERY TEXT "hello" FROM docs LIMIT 5');
  assert.strictEqual(route.method, 'POST');
  assert.strictEqual(route.path, '/collections/docs/points/query');
  assert.strictEqual(route.stmt_type, 'query');
  assert.ok(typeof route.payload === 'object');
});

test('listEmbeddingModels returns available models', () => {
  const models = nqql.listEmbeddingModels();
  assert.ok(Array.isArray(models));
  assert.ok(models.length > 0);
  assert.strictEqual(typeof models[0].name, 'string');
  assert.strictEqual(typeof models[0].dim, 'number');
});

// ============================================================================
console.log('\n========== F. Local Edge Executor ==========');

testAsync('localExecutor create collection, upsert, count, show collections', async () => {
  fs.mkdirSync(TEST_DIR, { recursive: true });
  const client = nqql.localExecutor(TEST_DIR, { onDiskPayload: false });

  const r1 = await client.execute('CREATE COLLECTION edge_test');
  assert.strictEqual(r1.ok, true);

  const r2 = await client.execute('SHOW COLLECTIONS');
  assert.strictEqual(r2.ok, true);

  const r3 = await client.execute('COUNT FROM edge_test');
  assert.strictEqual(r3.ok, true);

  await client.close();
});

// ============================================================================
console.log('\n========== G. HTTP embedding (executeStmt via local mock) ==========');

// The JS wrapper exports `executeStmt` and `httpExecutor` unconditionally, so
// symbol-existence checks cannot prove the compiled native binding honors HTTP
// embedding. This end-to-end test runs `executeStmt({ embedUrl })` against a
// local Node mock and asserts the embedding request actually went to it —
// requiring the native default-feature build (http-embedding) to be loaded.

testAsync('executeStmt() honors embedUrl via local mock HTTP endpoint', async () => {
  const embedding = [0.1, 0.2, 0.3, 0.4];
  let received = null;
  const server = http.createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
    });
    req.on('end', () => {
      received = {
        method: req.method,
        url: req.url,
        auth: req.headers.authorization,
        body,
      };
      const payload = JSON.stringify({
        data: [{ index: 0, embedding }],
      });
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(payload);
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const dataDir = path.join(os.tmpdir(), 'nqql_edge_http_' + Date.now());
  fs.mkdirSync(dataDir, { recursive: true });

  try {
    const [stmt] = nqql.parse("UPSERT INTO http_docs VALUES {id: 1, text: 'hello'}");
    const report = await nqql.executeStmt(stmt, {
      dataDir,
      onDiskPayload: false,
      embedUrl: `http://127.0.0.1:${server.address().port}/v1/embeddings`,
      embedKey: 'test-key',
      embedModel: 'mock-embed',
      embedDim: 4,
    });
    assert.strictEqual(report.ok, true, JSON.stringify(report));
    assert.ok(received, 'mock embedding endpoint must receive a request');
    assert.strictEqual(received.method, 'POST');
    assert.strictEqual(received.url, '/v1/embeddings');
    assert.strictEqual(received.auth, 'Bearer test-key');
    const body = JSON.parse(received.body);
    assert.strictEqual(body.model, 'mock-embed');
    assert.deepStrictEqual(body.input, ['hello']);
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true });
    await new Promise((resolve) => server.close(resolve));
  }
});

// ============================================================================
console.log(`\n========================================`);
console.log(`  TEST RESULTS: ${passed} passed, ${failed} failed.`);
console.log(`========================================\n`);

if (failed > 0) {
  process.exitCode = 1;
}
