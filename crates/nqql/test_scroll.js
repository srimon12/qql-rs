'use strict';

/**
 * Tests for scrollCursor / scrollStream (shared DX layer).
 * Network-free: a counting fake client serves fixed pages through the real
 * ExecutionReport class, so pagination, laziness, and backpressure are
 * asserted without a Qdrant server.
 */

const { test } = require('node:test');
const assert = require('node:assert/strict');
const sdk = require('./index.js');

function rows(ids) {
  return ids.map((id) => ({ id, score: 1, payload: { tag: `p-${id}` } }));
}

function scrollReport(data) {
  return new sdk.ExecutionReport({
    ok: true,
    succeeded: 1,
    failed: 0,
    results: [{ type: 'scroll', status: 'success', data }],
  });
}

/** Fake client over fixed pages; records every (sql, options) call. */
function fakeClient(pages) {
  const calls = [];
  let n = 0;
  return {
    calls,
    async execute(sql, options) {
      calls.push({ sql, options });
      return scrollReport(pages[n++] ?? []);
    },
  };
}

async function collect(iterable) {
  const out = [];
  for await (const point of iterable) {
    out.push(point);
  }
  return out;
}

test('scrollCursor/scrollStream are exported', () => {
  assert.strictEqual(typeof sdk.scrollCursor, 'function');
  assert.strictEqual(typeof sdk.scrollStream, 'function');
});

test('lazy pagination across pages, stops on empty page', async () => {
  const client = fakeClient([rows([1, 2]), rows([3]), []]);
  const points = await collect(sdk.scrollCursor(client, 'docs', { batchSize: 2 }));
  assert.deepStrictEqual(points.map((p) => p.id), [1, 2, 3]);
  assert.strictEqual(client.calls.length, 3);
  // First page has no AFTER clause and no params.
  assert.ok(!client.calls[0].sql.includes('AFTER'));
  assert.strictEqual(client.calls[0].options, undefined);
  // Later pages bind the last-seen id as :cursor.
  assert.ok(client.calls[1].sql.includes('AFTER :cursor'));
  assert.deepStrictEqual(client.calls[1].options, { params: { cursor: 2 } });
  assert.deepStrictEqual(client.calls[2].options, { params: { cursor: 3 } });
  // Points are typed ScoredPoints with payload accessors.
  assert.ok(points[0] instanceof sdk.ScoredPoint);
  assert.strictEqual(points[0].get('tag'), 'p-1');
});

test('empty collection stops after one page', async () => {
  const client = fakeClient([[]]);
  assert.deepStrictEqual(await collect(sdk.scrollCursor(client, 'docs')), []);
  assert.strictEqual(client.calls.length, 1);
});

test('where and batchSize shape the statement', async () => {
  const client = fakeClient([[]]);
  await collect(sdk.scrollCursor(client, 'docs', { batchSize: 7, where: 'price < 150.0' }));
  assert.strictEqual(client.calls.length, 1);
  assert.ok(client.calls[0].sql.includes('WHERE price < 150.0'));
  assert.ok(client.calls[0].sql.includes('LIMIT 7'));
  // Clause order follows the grammar: WHERE before LIMIT.
  assert.ok(
    client.calls[0].sql.indexOf('WHERE') < client.calls[0].sql.indexOf('LIMIT'),
  );
});

test('withPayload defaults to true; false strips payload client-side', async () => {
  const kept = await collect(sdk.scrollCursor(fakeClient([rows([1]), []]), 'docs'));
  assert.deepStrictEqual(kept[0].payload, { tag: 'p-1' });

  const stripped = await collect(
    sdk.scrollCursor(fakeClient([rows([1]), []]), 'docs', { withPayload: false }),
  );
  assert.strictEqual(stripped[0].payload, null);
  assert.strictEqual(stripped[0].id, 1);
});

test('withVector appends WITH VECTOR only when true', async () => {
  const plain = fakeClient([[]]);
  await collect(sdk.scrollCursor(plain, 'docs'));
  assert.ok(!plain.calls[0].sql.includes('WITH VECTOR'));

  const vectors = fakeClient([[]]);
  await collect(sdk.scrollCursor(vectors, 'docs', { withVector: true }));
  assert.ok(vectors.calls[0].sql.includes('WITH VECTOR'));
  // WITH VECTOR precedes LIMIT per the grammar order.
  assert.ok(
    vectors.calls[0].sql.indexOf('WITH VECTOR') < vectors.calls[0].sql.indexOf('LIMIT'),
  );
});

test('backpressure: slow consumer never buffers more than one page', async () => {
  const client = fakeClient([rows([1, 2]), rows([3, 4]), []]);
  const it = sdk.scrollCursor(client, 'docs', { batchSize: 2 })[Symbol.asyncIterator]();
  // First pull fetches exactly one page.
  assert.strictEqual((await it.next()).value.id, 1);
  assert.strictEqual(client.calls.length, 1);
  // Consuming the rest of page 1 fetches nothing new.
  assert.strictEqual((await it.next()).value.id, 2);
  assert.strictEqual(client.calls.length, 1);
  // Only crossing into page 2 triggers the second fetch.
  assert.strictEqual((await it.next()).value.id, 3);
  assert.strictEqual(client.calls.length, 2);
  assert.strictEqual((await it.next()).value.id, 4);
  assert.strictEqual(client.calls.length, 2);
  assert.strictEqual((await it.next()).done, true);
  assert.strictEqual(client.calls.length, 3);
});

test('scrollStream yields every point as a ReadableStream', async () => {
  const client = fakeClient([rows([1, 2]), rows([3]), []]);
  const stream = sdk.scrollStream(client, 'docs', { batchSize: 2 });
  assert.ok(stream instanceof ReadableStream);
  const ids = [];
  for await (const point of stream) {
    ids.push(point.id);
  }
  assert.deepStrictEqual(ids, [1, 2, 3]);
  assert.strictEqual(client.calls.length, 3);
});

test('scrollStream falls back without ReadableStream.from', async () => {
  const originalFrom = ReadableStream.from;
  assert.strictEqual(typeof originalFrom, 'function');
  ReadableStream.from = undefined;
  try {
    const stream = sdk.scrollStream(fakeClient([rows([1, 2]), []]), 'docs');
    assert.ok(stream instanceof ReadableStream);
    assert.deepStrictEqual(await drainIds(stream), [1, 2]);
  } finally {
    ReadableStream.from = originalFrom;
  }
  assert.strictEqual(typeof ReadableStream.from, 'function');
});

async function drainIds(stream) {
  const ids = [];
  const reader = stream.getReader();
  for (;;) {
    const step = await reader.read();
    if (step.done) break;
    ids.push(step.value.id);
  }
  reader.releaseLock();
  return ids;
}

test('option validation fails closed', async () => {
  const good = fakeClient([[]]);
  await assert.rejects(collect(sdk.scrollCursor(null, 'docs')), TypeError);
  await assert.rejects(collect(sdk.scrollCursor({}, 'docs')), TypeError);
  await assert.rejects(collect(sdk.scrollCursor(good, '')), TypeError);
  await assert.rejects(collect(sdk.scrollCursor(good, 42)), TypeError);
  await assert.rejects(collect(sdk.scrollCursor(good, 'docs', { batchSize: 0 })), TypeError);
  await assert.rejects(collect(sdk.scrollCursor(good, 'docs', { batchSize: 1.5 })), TypeError);
  await assert.rejects(collect(sdk.scrollCursor(good, 'docs', { where: 42 })), TypeError);
  await assert.rejects(
    collect(sdk.scrollCursor(good, 'docs', { withPayload: 'yes' })),
    TypeError,
  );
  await assert.rejects(collect(sdk.scrollCursor(good, 'docs', { withVector: 1 })), TypeError);
  // scrollStream validates synchronously.
  assert.throws(() => sdk.scrollStream(null, 'docs'), TypeError);
  assert.throws(() => sdk.scrollStream(good, ''), TypeError);
  assert.throws(() => sdk.scrollStream(good, 'docs', { batchSize: 0 }), TypeError);
});

test('escapes hyphenated and special collection names safely', async () => {
  const client = fakeClient([[]]);
  await collect(sdk.scrollCursor(client, 'my-hyphenated-collection'));
  assert.strictEqual(client.calls.length, 1);
  assert.ok(client.calls[0].sql.startsWith('SCROLL FROM "my-hyphenated-collection"'));
});

test('shardKey routes to string or numeric partition', async () => {
  const c1 = fakeClient([[]]);
  await collect(sdk.scrollCursor(c1, 'docs', { shardKey: 'tenant-a' }));
  assert.ok(c1.calls[0].sql.includes("SHARD 'tenant-a'"));

  const c2 = fakeClient([[]]);
  await collect(sdk.scrollCursor(c2, 'docs', { shardKey: 42 }));
  assert.ok(c2.calls[0].sql.includes('SHARD 42'));

  const c3 = fakeClient([[]]);
  await collect(sdk.scrollCursor(c3, 'docs', { shardKey: 100n }));
  assert.ok(c3.calls[0].sql.includes('SHARD 100'));

  const c4 = fakeClient([[]]);
  await assert.rejects(
    collect(sdk.scrollCursor(c4, 'docs', { shardKey: {} })),
    TypeError,
  );
  await assert.rejects(
    collect(sdk.scrollCursor(c4, 'docs', { shardKey: true })),
    TypeError,
  );
});

test('infinite loop guard terminates when cursor does not advance', async () => {
  const client = fakeClient([rows([1]), rows([1]), rows([1])]);
  const points = await collect(sdk.scrollCursor(client, 'docs'));
  assert.strictEqual(points.length, 1);
  assert.strictEqual(client.calls.length, 2);
});

test('Client prototype has scrollCursor and scrollStream delegates', async () => {
  assert.strictEqual(typeof sdk.Client.prototype.scrollCursor, 'function');
  assert.strictEqual(typeof sdk.Client.prototype.scrollStream, 'function');

  const client = fakeClient([rows([1]), []]);
  const points = await collect(sdk.Client.prototype.scrollCursor.call(client, 'docs'));
  assert.strictEqual(points.length, 1);
  assert.strictEqual(points[0].id, 1);

  const streamClient = fakeClient([rows([2]), []]);
  const stream = sdk.Client.prototype.scrollStream.call(streamClient, 'docs');
  assert.ok(stream instanceof ReadableStream);
  const ids = [];
  for await (const p of stream) {
    ids.push(p.id);
  }
  assert.deepStrictEqual(ids, [2]);
});

test('params option binds into where clause across pages', async () => {
  const client = fakeClient([rows([1]), rows([2]), []]);
  const points = await collect(
    sdk.scrollCursor(client, 'docs', {
      where: 'price < :max_price',
      params: { max_price: 150 },
      batchSize: 1,
    }),
  );
  assert.deepStrictEqual(points.map((p) => p.id), [1, 2]);
  assert.strictEqual(client.calls.length, 3);
  // Page 1: params contain max_price, no cursor
  assert.deepStrictEqual(client.calls[0].options, { params: { max_price: 150 } });
  // Page 2: params contain both max_price and cursor
  assert.deepStrictEqual(client.calls[1].options, {
    params: { max_price: 150, cursor: 1 },
  });
  // Page 3: params contain max_price and next cursor
  assert.deepStrictEqual(client.calls[2].options, {
    params: { max_price: 150, cursor: 2 },
  });
});

test('params validation fails on invalid types or reserved cursor key', async () => {
  const client = fakeClient([[]]);
  await assert.rejects(
    collect(sdk.scrollCursor(client, 'docs', { params: 'invalid' })),
    TypeError,
  );
  await assert.rejects(
    collect(sdk.scrollCursor(client, 'docs', { params: [1, 2] })),
    TypeError,
  );
  await assert.rejects(
    collect(sdk.scrollCursor(client, 'docs', { params: { cursor: 123 } })),
    TypeError,
  );
});


