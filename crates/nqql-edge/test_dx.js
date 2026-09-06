'use strict';

const assert = require('assert');
const nqql = require('./index.js');

console.log('Testing Node.js DX enhancements (nqql-edge)...');

// 1. Stmt.bind() and Stmt.toString()
{
  const stmts = nqql.parse('QUERY [1.0, 2.0] FROM docs WHERE status = :status LIMIT :limit');
  assert.strictEqual(stmts.length, 1);
  const stmt = stmts[0];

  assert.strictEqual(typeof stmt.bind, 'function');
  assert.strictEqual(typeof stmt.compileRoute, 'function');
  assert.strictEqual(typeof stmt.toString, 'function');

  // Test toString before binding
  const strBefore = stmt.toString();
  assert.ok(strBefore.includes(':status'));

  // Bind on statement
  const bound = stmt.bind({ status: 'active', limit: 10 });
  const strAfter = bound.toString();
  assert.ok(strAfter.includes("'active'"));
  assert.ok(strAfter.includes('LIMIT 10'));
  // Original stmt remains unchanged
  assert.ok(stmt.toString().includes(':status'));
}

// 2. Stmt.compileRoute(params)
{
  const stmts = nqql.parse('QUERY [0.1, 0.2] FROM items WHERE category = :cat LIMIT 5');
  const stmt = stmts[0];

  const route = stmt.compileRoute({ cat: 'books' });
  assert.strictEqual(route.method, 'POST');
  assert.ok(route.path.includes('/points/query'));
  assert.strictEqual(typeof route.payload, 'object');
  assert.strictEqual(route.payload.limit, 5);
}

// 3. Nested dictionary parameter expansion (:loc.lat, :loc.lon)
{
  const qql = 'QUERY [0.1, 0.2] FROM places WHERE lat = :loc.lat AND lon = :loc.lon';
  const bound = nqql.bind(qql, {
    loc: { lat: 37.7749, lon: -122.4194 }
  });
  assert.ok(bound.includes('37.7749'));
  assert.ok(bound.includes('-122.4194'));

  // On Stmt as well
  const stmt = nqql.parse(qql)[0];
  const boundStmt = stmt.bind({ loc: { lat: 37.7749, lon: -122.4194 } });
  assert.ok(boundStmt.toString().includes('37.7749'));
}

// 4. Vector truncation in bind() and Stmt.toString()
{
  const qql = 'QUERY [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] FROM docs WHERE id = :id';
  const boundTrunc = nqql.bind(qql, { id: 42 }, { truncateVectors: true });
  assert.ok(boundTrunc.includes('... (10 dims)'));

  const boundNoTrunc = nqql.bind(qql, { id: 42 }, { truncateVectors: false });
  assert.ok(!boundNoTrunc.includes('dims)'));
  assert.ok(boundNoTrunc.includes('[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]'));

  // Truncate vectors on Stmt.bind
  const stmt = nqql.parse(qql)[0];
  const boundStmt = stmt.bind({ id: 42 }, { truncateVectors: true });
  assert.ok(boundStmt.toString().includes('... (10 dims)'));
}

// 5. ExecutionReport and ScoredPoint
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
          { id: 'uuid-2', score: 0.82, payload: { name: 'second' } }
        ]
      },
      {
        type: 'facet',
        status: 'success',
        data: [
          { value: 'red', count: 12 },
          { value: 'blue', count: 8 }
        ]
      },
      {
        type: 'count',
        status: 'success',
        message: 'Count: 42',
        data: { result: { count: 42 } }
      }
    ]
  };

  const report = new nqql.ExecutionReport(mockPayload);
  assert.strictEqual(report.ok, true);
  assert.strictEqual(report.succeeded, 3);
  assert.strictEqual(report.failed, 0);

  const hits = report.hits(0);
  assert.strictEqual(hits.length, 2);
  assert.strictEqual(hits[0].id, 1);
  assert.strictEqual(hits[0].score, 0.95);
  assert.strictEqual(hits[0].payload.name, 'first');
  assert.strictEqual(hits[0].get('name'), 'first');
  assert.deepStrictEqual(hits[0].vector, [0.1, 0.2]);
  assert.strictEqual(hits[1].id, 'uuid-2');
  assert.strictEqual(hits[1].score, 0.82);

  // Facet report
  assert.strictEqual(report.facet(1).length, 2);
  assert.strictEqual(report.facet(1)[0].value, 'red');

  // Count report
  assert.strictEqual(report.count(2), 42);
}

// 6. executeHits wrapper check
{
  assert.strictEqual(typeof nqql.executeHits, 'function');
  assert.strictEqual(typeof nqql.Client.prototype.executeHits, 'function');
}

console.log('All nqql-edge DX unit tests passed successfully!');
