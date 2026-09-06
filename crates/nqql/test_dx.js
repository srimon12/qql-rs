'use strict';

// ============================================================================
// DX test suite for the prepared-statement / parameter surface.
// Network-free: parse, bind, compileRoute, toString, and report accessors.
// Mirrors crates/pyqql/tests/test_dx.py (the cross-SDK parity spec).
// ============================================================================

const assert = require('assert');
const sdk = require('./index.js');

const LABEL = process.argv[2] || 'nqql';
console.log(`Testing Node.js DX enhancements (${LABEL})...`);

// 1. Stmt.bind() — named params, immutability, params optional
{
  const stmts = sdk.parse('QUERY [1.0, 2.0] FROM docs WHERE status = :status LIMIT :limit');
  assert.strictEqual(stmts.length, 1);
  const stmt = stmts[0];

  assert.strictEqual(typeof stmt.bind, 'function');
  assert.strictEqual(typeof stmt.compileRoute, 'function');
  assert.strictEqual(typeof stmt.toString, 'function');
  assert.strictEqual(typeof stmt.toReadableString, 'function');

  // toString is the full canonical form (mirrors Python str(stmt)) and re-parses.
  const before = stmt.toString();
  assert.ok(before.includes(':status'));
  assert.strictEqual(sdk.parse(before).length, 1);

  // Bind on statement
  const bound = stmt.bind({ status: 'active', limit: 10 });
  const after = bound.toString();
  assert.ok(after.includes("'active'"));
  assert.ok(after.includes('LIMIT 10'));
  // Original stmt remains unchanged
  assert.ok(stmt.toString().includes(':status'));

  // bind() without params is a no-op (mirrors pyqql bind(stmt, None))
  const untouched = stmt.bind();
  assert.strictEqual(untouched.toString(), before);

  // Invalid params types fail closed (mirrors pyqql ValueError)
  assert.throws(() => stmt.bind(42), /params must be an object/);
}

// 2. Stmt.toString() / toReadableString() truncation split
{
  const stmt = sdk.parse('QUERY [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] FROM docs')[0];
  // Full form keeps every dim and re-parses (Python str parity).
  const full = stmt.toString();
  assert.ok(full.includes('1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0'));
  assert.strictEqual(sdk.parse(full).length, 1);
  // Readable preview truncates (Python repr parity) and does NOT re-parse.
  const readable = stmt.toReadableString();
  assert.ok(readable.includes('... (10 dims)'));
  assert.throws(() => sdk.parse(readable));
}

// 3. Stmt.compileRoute(params)
{
  const stmt = sdk.parse('QUERY [0.1, 0.2] FROM items WHERE category = :cat LIMIT 5')[0];
  const route = stmt.compileRoute({ cat: 'books' });
  assert.strictEqual(route.method, 'POST');
  assert.ok(route.path.includes('/points/query'));
  assert.strictEqual(typeof route.payload, 'object');
  assert.strictEqual(route.payload.limit, 5);
  assert.ok(JSON.stringify(route.payload).includes('books'));
}

// 4. Nested dictionary parameter expansion (:loc.lat, :loc.lon)
{
  const qql = 'QUERY [0.1, 0.2] FROM places WHERE lat = :loc.lat AND lon = :loc.lon';
  const bound = sdk.bind(qql, { loc: { lat: 37.7749, lon: -122.4194 } });
  assert.ok(bound.includes('37.7749'));
  assert.ok(bound.includes('-122.4194'));

  // On Stmt as well (module bind accepts Stmt → returns Stmt)
  const stmt = sdk.parse(qql)[0];
  const boundStmt = sdk.bind(stmt, { loc: { lat: 37.7749, lon: -122.4194 } });
  assert.ok(boundStmt instanceof sdk.Stmt);
  assert.ok(boundStmt.toString().includes('37.7749'));
}

// 5. Vector truncation on string bind and module bind(Stmt)
{
  const qql = 'QUERY [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] FROM docs WHERE id = :id';
  const boundTrunc = sdk.bind(qql, { id: 42 }, { truncateVectors: true });
  assert.ok(boundTrunc.includes('... (10 dims)'));

  const boundNoTrunc = sdk.bind(qql, { id: 42 }, { truncateVectors: false });
  assert.ok(!boundNoTrunc.includes('dims)'));
  assert.ok(boundNoTrunc.includes('1, 2, 3, 4, 5, 6, 7, 8, 9, 10'));

  // bind(stmt, params) returns a Stmt; with truncateVectors → readable string.
  const stmt = sdk.parse(qql)[0];
  const boundStmt = sdk.bind(stmt, { id: 42 });
  assert.ok(boundStmt instanceof sdk.Stmt);
  const readable = sdk.bind(stmt, { id: 42 }, { truncateVectors: true });
  assert.strictEqual(typeof readable, 'string');
  assert.ok(readable.includes('... (10 dims)'));
}

// 6. bind() without params returns input unchanged
{
  const qql = 'QUERY [0.1] FROM docs WHERE x = :x';
  assert.strictEqual(sdk.bind(qql), qql);
  assert.strictEqual(sdk.bind(qql, undefined), qql);
  const stmt = sdk.parse(qql)[0];
  assert.strictEqual(sdk.bind(stmt).toString(), stmt.toString());
}

// 7. Error codes surface with .code (QQL-BIND-*)
{
  // bind() without params is a no-op — pass an empty dict to exercise lookup.
  try {
    sdk.bind('QUERY [0.1] FROM docs WHERE x = :missing', {});
    assert.fail('expected QQL-BIND-MISSING-PARAM');
  } catch (err) {
    assert.match(err.message, /missing value for named parameter/);
    assert.strictEqual(err.code, 'QQL-BIND-MISSING-PARAM');
  }
  try {
    sdk.bind('QUERY TEXT :q FROM docs LIMIT ?;', { q: 'x' });
    assert.fail('expected QQL-BIND-MIXED-STYLE');
  } catch (err) {
    assert.strictEqual(err.code, 'QQL-BIND-MIXED-STYLE');
  }
  try {
    sdk.bind('QUERY TEXT ? FROM docs', ['a', 'b']);
    assert.fail('expected QQL-BIND-UNUSED-PARAMS');
  } catch (err) {
    assert.strictEqual(err.code, 'QQL-BIND-UNUSED-PARAMS');
  }
}

// 8. ExecutionReport and ScoredPoint (mocked report — pyqql parity)
{
  const mockPayload = {
    ok: true,
    succeeded: 3,
    failed: 0,
    results: [
      {
        type: 'query',
        status: 'success',
        data: [
          { id: 1, score: 0.95, payload: { name: 'first' }, vector: [0.1, 0.2] },
          { id: 'uuid-2', score: 0.82, payload: { name: 'second' } },
          'garbage-entry-is-filtered',
        ],
      },
      {
        type: 'facet',
        status: 'success',
        data: [{ value: 'red', count: 12 }, { value: 'blue', count: 8 }],
      },
      {
        type: 'count',
        status: 'success',
        message: 'Count: 42',
        data: { result: { count: 42 } },
      },
    ],
  };

  const report = new sdk.ExecutionReport(mockPayload);
  assert.strictEqual(report.ok, true);
  assert.strictEqual(report.succeeded, 3);
  assert.strictEqual(report.failed, 0);

  const hits = report.hits(0);
  assert.strictEqual(hits.length, 2); // non-object entries filtered (pyqql parity)
  assert.strictEqual(hits[0].id, 1);
  assert.strictEqual(hits[0].score, 0.95);
  assert.strictEqual(hits[0].payload.name, 'first');
  assert.strictEqual(hits[0].get('name'), 'first');
  assert.deepStrictEqual(hits[0].vector, [0.1, 0.2]);
  assert.strictEqual(hits[1].id, 'uuid-2');
  assert.strictEqual(hits[1].score, 0.82);
  assert.strictEqual(hits[0].payload, mockPayload.results[0].data[0].payload);

  // points() alias + negative index (Python list semantics: -1 = last stmt)
  assert.strictEqual(report.points(0).length, 2);
  assert.strictEqual(report.points(-3).length, 2);
  assert.strictEqual(report.points(-1).length, 0); // last stmt is the count result
  // Out-of-range → empty
  assert.deepStrictEqual(report.hits(9), []);

  // Defaults mirror pyqql when keys are absent
  const empty = new sdk.ExecutionReport({});
  assert.strictEqual(empty.ok, false);
  assert.deepStrictEqual(empty.results, []);
  assert.strictEqual(empty.succeeded, 0);
  assert.strictEqual(empty.failed, 0);
  assert.deepStrictEqual(empty.hits(), []);
  assert.strictEqual(empty.count(), 0);

  // Facet report
  assert.strictEqual(report.facet(1).length, 2);
  assert.strictEqual(report.facet(1)[0].value, 'red');

  // Count report
  assert.strictEqual(report.count(2), 42);
}

// 9. executeHits wrapper check
{
  assert.strictEqual(typeof sdk.executeHits, 'function');
  assert.strictEqual(typeof sdk.Client.prototype.executeHits, 'function');
}

console.log(`All ${LABEL} DX unit tests passed successfully!`);
