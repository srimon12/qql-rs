/**
 * QQL 5-line happy path (Node.js / nqql) — offline, no Qdrant server.
 *
 * Runs in CI: parse → hybrid CTE as text → tenant isolation → bind →
 * compile → `:rows` ingest shape. Live execution (`client.execute`,
 * `client.upsertMany`) needs a server and is covered by live integration
 * tests, not here.
 *
 * Usage: node examples/quickstart.mjs [path/to/nqql/package]
 */
const path = process.argv[2] || 'crates/nqql';
const { createRequire } = await import('module');
const require = createRequire(import.meta.url);
const sdk = require(`../${path}/index.js`);
const assert = (await import('assert')).default;

// 1. One language for complex retrieval — hybrid fusion as text.
const q = `
WITH
  dense  AS (QUERY TEXT 'vector databases' FROM docs USING dense  LIMIT 100),
  sparse AS (QUERY TEXT 'vector databases' FROM docs USING sparse LIMIT 100)
QUERY FUSION RRF FROM docs PREFETCH (dense, sparse) LIMIT 10
`;
assert.strictEqual(sdk.isValid(q), true);

// 2. Tenant isolation: injectFilter always; SHARD routes.
const [stmt] = sdk.parse(q);
stmt.injectFilter('tenant_id', '=', 'acme');
stmt.shardKey = 'acme';

// 3. Bind + compile offline — the exact REST route, no I/O.
const route = sdk.compileQuery(
  'QUERY TEXT :q FROM docs WHERE tenant_id = :t LIMIT :lim',
  { q: 'vector databases', t: 'acme', lim: 10 },
);
assert.strictEqual(route.method, 'POST');
assert.strictEqual(route.path, '/collections/docs/points/query');

// 4. Ingest shape is data, not text: point dicts splice into `:rows`.
// Live: await client.upsertMany('docs', rows, { batchSize: 100 }).
const rows = [
  { id: 1, vector: { dense: [0.1, 0.2, 0.3] }, tag: 'a' },
  { id: 2, vector: { dense: [0.4, 0.5, 0.6] }, tag: 'b' },
];
const [tpl] = sdk.parse('UPSERT INTO docs VALUES :rows');
const bound = tpl.bind({ rows });
assert.ok(bound.toString().includes('id: 1'));
assert.ok(bound.toString().includes('id: 2'));
assert.strictEqual(typeof sdk.Client.prototype.upsertMany, 'function');

console.log('quickstart ok: hybrid + isolation + bind + :rows');
