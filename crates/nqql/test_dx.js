'use strict';

// ============================================================================
// DX test suite for the prepared-statement / parameter surface.
// Offline-first: parse, bind, compileRoute, toString, and report accessors.
// The §11 transport check needs no Qdrant — it points at a dead port, so a
// transport error (not a bind error) proves typed-array params bound fine.
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

  // Re-binding an already-bound Stmt raises QQL-BIND-ALREADY-BOUND
  assert.throws(() => bound.bind({ status: 'inactive' }), (err) => {
    return err.code === 'QQL-BIND-ALREADY-BOUND';
  });
  // Calling bind() without params on an already-bound Stmt is a no-op
  assert.strictEqual(bound.bind().toString(), after);

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

  // compileRoute with new params on an already-bound Stmt raises QQL-BIND-ALREADY-BOUND
  const boundCat = stmt.bind({ cat: 'music' });
  assert.throws(() => boundCat.compileRoute({ cat: 'art' }), (err) => {
    return err.code === 'QQL-BIND-ALREADY-BOUND';
  });
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
        ok: true,
        operation: 'QUERY',
        message: 'Found 2 hits',
        data: [
          { id: 1, score: 0.95, payload: { name: 'first' }, vector: [0.1, 0.2] },
          { id: 'uuid-2', score: 0.82, payload: { name: 'second' } },
        ],
      },
      {
        ok: true,
        operation: 'FACET',
        message: 'Found 2 facet hit(s)',
        data: [{ value: 'red', count: 12 }, { value: 'blue', count: 8 }],
      },
      {
        ok: true,
        operation: 'COUNT',
        message: 'Count: 42',
        data: { count: 42 },
      },
    ],
  };

  const report = new sdk.ExecutionReport(mockPayload);
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
  assert.strictEqual(hits[1].vector, null);
  assert.strictEqual(hits[1].shard_key, null);
  assert.strictEqual(hits[0].payload, mockPayload.results[0].data[0].payload);

  // ids() accessor
  assert.deepStrictEqual(report.ids(0), [1, 'uuid-2']);

  // points() alias + negative index (Python list semantics: -1 = last stmt)
  assert.strictEqual(report.points(0).length, 2);
  assert.strictEqual(report.points(-3).length, 2);
  assert.strictEqual(report.points(-1).length, 0); // last stmt is the count result
  // Out-of-range → empty
  assert.deepStrictEqual(report.hits(9), []);
  assert.deepStrictEqual(report.hits(-10), []);
  assert.deepStrictEqual(report.points(-10), []);

  // Defaults mirror pyqql when keys are absent
  const empty = new sdk.ExecutionReport({});
  assert.strictEqual(empty.ok, false);
  assert.deepStrictEqual(empty.results, []);
  assert.strictEqual(empty.succeeded, 0);
  assert.strictEqual(empty.failed, 0);
  assert.deepStrictEqual(empty.hits(), []);
  assert.strictEqual(empty.count(), 0);
  assert.deepStrictEqual(empty.groups(), []);

  // Grouped query (GROUP BY): canonical `{groups: [...]}` payloads only.
  assert.deepStrictEqual(
    report.groups(9),
    [],
    'out-of-range stmt yields empty groups'
  );
  const grouped = new sdk.ExecutionReport({
    ok: true,
    succeeded: 1,
    failed: 0,
    results: [
      {
        ok: true,
        operation: 'QUERY_GROUPS',
        message: 'Found 2 group(s)',
        data: {
          groups: [
            { id: 'a', hits: [{ id: 1, score: 0.9 }] },
            { id: 'b', hits: [{ id: 2, score: 0.8 }] },
          ],
        },
      },
    ],
  });
  assert.strictEqual(grouped.groups().length, 2);
  assert.strictEqual(grouped.groups()[0].id, 'a');
  assert.strictEqual(grouped.groups()[1].hits.length, 1);
  // A non-groups operation never guesses.
  assert.deepStrictEqual(report.groups(1), []);

  // Group hits are ScoredPoint instances (pyqql parity).
  assert.ok(grouped.groups()[0].hits[0] instanceof sdk.ScoredPoint);
  assert.strictEqual(grouped.groups()[0].hits[0].id, 1);

  // get() resolves attributes first, then payload keys, then the default.
  assert.strictEqual(hits[0].get('id'), 1);
  assert.strictEqual(hits[0].get('score'), 0.95);
  assert.strictEqual(hits[0].get('name'), 'first');
  assert.deepStrictEqual(hits[0].get('vector'), [0.1, 0.2]);
  assert.strictEqual(hits[0].get('missing', 'dflt'), 'dflt');
  assert.strictEqual(hits[1].get('vector', 'dflt'), 'dflt'); // null vector falls back

  // withoutPayload() strips the payload, keeping id/score (original untouched).
  const stripped = hits[0].withoutPayload();
  assert.ok(stripped instanceof sdk.ScoredPoint);
  assert.strictEqual(stripped.payload, null);
  assert.strictEqual(stripped.text, null);
  assert.strictEqual(stripped.id, 1);
  assert.strictEqual(hits[0].payload.name, 'first');

  // Facet report
  assert.strictEqual(report.facet(1).length, 2);
  assert.strictEqual(report.facet(1)[0].value, 'red');

  // Count report
  assert.strictEqual(report.count(2), 42);
}

// 8b. Metadata accessors: collections / collection / shardKeys / quotas
// (pyqql parity — SHOW_* results need no hand-digging into results[].data).
{
  const meta = new sdk.ExecutionReport({
    ok: true,
    succeeded: 4,
    failed: 0,
    results: [
      {
        ok: true,
        operation: 'SHOW_COLLECTIONS',
        message: 'Collections listed',
        data: { collections: ['docs', 'articles'] },
      },
      {
        ok: true,
        operation: 'SHOW_COLLECTION',
        message: 'Collection shown',
        data: { status: 'green', points_count: 42 },
      },
      {
        ok: true,
        operation: 'SHOW_SHARD_KEYS',
        message: 'Shard keys listed',
        data: { shard_keys: ['acme', 101] },
      },
      {
        ok: true,
        operation: 'SHOW_QUOTAS',
        message: 'Quotas shown',
        data: { enabled: true },
      },
    ],
  });

  assert.deepStrictEqual(meta.collections(), ['docs', 'articles']);
  assert.deepStrictEqual(meta.collections(0), ['docs', 'articles']);
  assert.deepStrictEqual(meta.collections(1), []); // wrong op → empty
  assert.deepStrictEqual(meta.collections(9), []);
  assert.deepStrictEqual(meta.collection(1), { status: 'green', points_count: 42 });
  assert.strictEqual(meta.collection(0), null);
  assert.deepStrictEqual(meta.shardKeys(2), ['acme', 101]);
  assert.deepStrictEqual(meta.shardKeys(-2), ['acme', 101]); // negative index
  assert.deepStrictEqual(meta.shardKeys(0), []);
  assert.deepStrictEqual(meta.quotas(3), { enabled: true });
  assert.strictEqual(meta.quotas(0), null);

  // SET_QUOTA answers the quotas() accessor too.
  const setQuota = new sdk.ExecutionReport({
    ok: true,
    succeeded: 1,
    failed: 0,
    results: [
      { ok: true, operation: 'SET_QUOTA', message: 'ok', data: { enabled: false } },
    ],
  });
  assert.deepStrictEqual(setQuota.quotas(), { enabled: false });
}

// 9. executeHits wrapper check
{
  assert.strictEqual(typeof sdk.executeHits, 'function');
  assert.strictEqual(typeof sdk.Client.prototype.executeHits, 'function');
}

// 10. Typed-array vector params bind identically to plain arrays.
{
  const vec = [0.1, 0.2, 0.3, 0.4];
  const f64 = new Float64Array(vec);
  const f32 = new Float32Array(vec);
  const q = 'QUERY :v FROM docs USING dense LIMIT 2';

  // Stmt.bind equivalence (typed === plain, exact same canonical string).
  const fromPlain = sdk.parse(q)[0].bind({ v: vec }).toString();
  assert.ok(fromPlain.includes('[0.1, 0.2, 0.3, 0.4]'));
  assert.strictEqual(sdk.parse(q)[0].bind({ v: f64 }).toString(), fromPlain);
  assert.strictEqual(sdk.parse(q)[0].bind({ v: f32 }).toString(), fromPlain);

  // Module bind + compile equivalence.
  assert.strictEqual(sdk.bind(q, { v: f64 }), sdk.bind(q, { v: vec }));
  assert.deepStrictEqual(sdk.compile(q, { v: f64 }), sdk.compile(q, { v: vec }));

  // Positional ? with typed arrays.
  const qp = 'QUERY ? FROM docs USING dense LIMIT 1';
  assert.strictEqual(
    sdk.parse(qp)[0].bind([f64]).toString(),
    sdk.parse(qp)[0].bind([vec]).toString()
  );

  // Flat {data, dim} multivector re-parses to the same statement as nested
  // lists (string bind renders the dict literally; the parser chunks it).
  const flat = sdk.bind('QUERY VECTOR :m FROM docs USING dense', {
    m: { data: [0.1, 0.2, 0.3, 0.4], dim: 2 },
  });
  const nested = sdk.bind('QUERY VECTOR :m FROM docs USING dense', {
    m: [[0.1, 0.2], [0.3, 0.4]],
  });
  assert.strictEqual(sdk.parse(flat)[0].toString(), sdk.parse(nested)[0].toString());

  // Sparse indices as Uint32Array, values as Float64Array.
  const sp1 = sdk.bind('QUERY VECTOR :s FROM docs USING sparse', {
    s: { indices: [1, 5], values: [0.5, 0.8] },
  });
  const sp2 = sdk.bind('QUERY VECTOR :s FROM docs USING sparse', {
    s: { indices: new Uint32Array([1, 5]), values: new Float64Array([0.5, 0.8]) },
  });
  assert.strictEqual(sp2, sp1);

  // Raw binary without a float dtype fails closed with guidance.
  assert.throws(
    () => sdk.parse(q)[0].bind({ v: Buffer.from([1, 2, 3, 4]) }),
    /Float32Array or Float64Array/
  );
}


// upsertMany surface (offline): bulk ingest lives on the client next to
// execute — one `:rows` template prepared once, no hand-rolled batch loops.
{
  assert.strictEqual(typeof sdk.Client.prototype.upsertMany, 'function');
}

// normalizeUpsertRows (offline): typed arrays convert like Python and WASM,
// raw binary fails closed with wrap-first guidance.
{
  const dx = require('./dx-common.js');
  assert.strictEqual(typeof dx.normalizeUpsertRows, 'function');
  const converted = dx.normalizeUpsertRows([
    { id: 1, vector: { dense: new Float32Array([0.1, 0.2]) } },
    { id: 2, vector: { sparse: { indices: new Uint32Array([1, 5]), values: new Float64Array([0.5, 0.8]) } } },
  ]);
  assert.ok(Array.isArray(converted[0].vector.dense));
  assert.deepStrictEqual(converted[1].vector.sparse.indices, [1, 5]);
  assert.strictEqual(converted[1].vector.sparse.values.length, 2);
  assert.throws(
    () => dx.normalizeUpsertRows([{ id: 1, vector: Buffer.from([1, 2, 3]) }]),
    /Float32Array or Float64Array/,
  );
  assert.throws(
    () => dx.normalizeUpsertRows([{ id: 1, vector: new ArrayBuffer(8) }]),
    /Float32Array or Float64Array/,
  );
  // Non-array top level passes through for the native QQL-BIND-TYPE-MISMATCH.
  assert.strictEqual(dx.normalizeUpsertRows('nope'), 'nope');
}

// 12. u64 exactness at the JS boundary (WASM parity: safe ints stay
// Number, snowflake u64 crosses as BigInt, BigInt binds back exactly,
// unsafe Numbers fail closed naming BigInt — never silent rounding).
{
  const SNOWFLAKE = 1479834607549681654n;
  const DIGITS = '1479834607549681654';

  // count() tolerates a BigInt count (u64 crosses as BigInt) and Numbers.
  const bigCount = new sdk.ExecutionReport({
    ok: true,
    succeeded: 1,
    failed: 0,
    results: [{ ok: true, operation: 'COUNT', message: 'Count: 8317', data: { count: 8317n } }],
  });
  assert.strictEqual(bigCount.count(), 8317);
  const numCount = new sdk.ExecutionReport({
    ok: true,
    succeeded: 1,
    failed: 0,
    results: [{ ok: true, operation: 'COUNT', message: 'Count: 7', data: { count: 7 } }],
  });
  assert.strictEqual(numCount.count(), 7);

  // BigInt cursor binds exactly (scroll cursors round-trip snowflake IDs).
  const cursorStmt = sdk.parse('SCROLL FROM docs AFTER :cursor LIMIT 3')[0];
  const boundCursor = cursorStmt.bind({ cursor: SNOWFLAKE });
  assert.ok(boundCursor.toString().includes(DIGITS));

  // Unsafe integer Numbers fail closed naming BigInt — never silently rounded.
  assert.throws(() => cursorStmt.bind({ cursor: Number(SNOWFLAKE) }), /BigInt/);
  assert.throws(
    () => sdk.bind('SCROLL FROM docs AFTER :cursor LIMIT 3', { cursor: Number(SNOWFLAKE) }),
    /BigInt/,
  );

  // Safe integers still bind as before (back-compat).
  assert.ok(cursorStmt.bind({ cursor: 42 }).toString().includes('AFTER 42'));

  // Stmt.toObject keeps the snowflake exact: a BigInt, never a rounded Number.
  const obj = sdk.parse(`QUERY POINTS (${DIGITS}) FROM docs`)[0].toObject();
  const seen = JSON.stringify(obj, (_, v) => (typeof v === 'bigint' ? `BIG:${v}` : v));
  assert.ok(seen.includes(`BIG:${DIGITS}`), `snowflake must cross as BigInt: ${seen.slice(0, 200)}`);

  // Small IDs stay plain Numbers in toObject.
  const smallSeen = JSON.stringify(
    sdk.parse('QUERY POINTS (3176) FROM docs')[0].toObject(),
    (_, v) => (typeof v === 'bigint' ? `BIG:${v}` : v),
  );
  assert.ok(smallSeen.includes('3176') && !smallSeen.includes('BIG:'));

  // Numeric shard keys read back as BigInt (exact on both sides of u64).
  const sharded = sdk.parse('SCROLL FROM docs LIMIT 1')[0];
  sharded.shardKey = SNOWFLAKE;
  assert.strictEqual(sharded.shardKey, SNOWFLAKE);
}

// 11. Typed-array execute params survive the napi boundary (P0-1).
// Server SDK only (`LABEL === 'nqql'`): binding runs before any I/O, so a
// dead port proves the params bound — a transport error means the
// Float32Array bound fine, while a bind error would mean it mangled.
(async () => {
  if (LABEL === 'nqql') {
    const probing = new sdk.Client({ url: 'http://127.0.0.1:9' });
    const transportErr = await probing
      .execute('QUERY :v FROM docs USING dense LIMIT 1', {
        params: { v: new Float32Array([0.1, 0.2, 0.3, 0.4]) },
      })
      .then(
        () => null,
        (err) => err,
      );
    assert.ok(transportErr, 'expected a transport error, not success');
    assert.ok(
      transportErr.code && transportErr.code.startsWith('QQL-TRANSPORT'),
      `expected QQL-TRANSPORT-*, got ${transportErr.code}: ${transportErr.message}`,
    );
    // Control: missing params still fail closed before any I/O.
    const bindErr = await probing
      .execute('QUERY :v FROM docs USING dense LIMIT 1', {})
      .then(
        () => null,
        (err) => err,
      );
    assert.ok(bindErr, 'expected a bind error, not success');
    assert.strictEqual(bindErr.code, 'QQL-BIND-MISSING-PARAM');
    await probing.close();
  }

  console.log(`All ${LABEL} DX unit tests passed successfully!`);
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
