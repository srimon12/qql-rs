"use strict";

/**
 * Wrapper-only tests for nqql-edge option normalization.
 * No native binding required — run via `npm run test:options`.
 */

const { test } = require("node:test");
const assert = require("node:assert");
const {
  normalizeLocalOptions,
  normalizeStandaloneOptions,
} = require("./options.js");

test("localExecutor forwards sparse/multi/image/reranker model slots", () => {
  const opts = normalizeLocalOptions({
    onDiskPayload: false,
    model: "BGESmallENV15",
    sparseModel: "splade",
    multiModel: "bge-m3",
    imageModel: "clip-vision",
    rerankerModel: "bge-reranker-base",
    cacheDir: "/var/cache",
    showDownloadProgress: true,
  });
  assert.deepStrictEqual(opts, {
    onDiskPayload: false,
    model: "BGESmallENV15",
    sparseModel: "splade",
    multiModel: "bge-m3",
    imageModel: "clip-vision",
    rerankerModel: "bge-reranker-base",
    cacheDir: "/var/cache",
    showDownloadProgress: true,
    walSegmentMb: undefined,
    bm25K1: undefined,
    bm25B: undefined,
    bm25AvgLen: undefined,
  });
});

test("localExecutor forwards bm25 document params raw for native validation", () => {
  const opts = normalizeLocalOptions({
    bm25K1: 2.0,
    bm25B: 0.5,
    bm25AvgLen: 8,
  });
  assert.strictEqual(opts.bm25K1, 2.0);
  assert.strictEqual(opts.bm25B, 0.5);
  assert.strictEqual(opts.bm25AvgLen, 8);
  // Invalid values are NOT dropped here — the Rust side rejects them with
  // QQL-VALIDATION-CONFIG instead of silently using the defaults.
  assert.strictEqual(normalizeLocalOptions({ bm25K1: 0 }).bm25K1, 0);
  assert.strictEqual(normalizeLocalOptions({ bm25B: -1 }).bm25B, -1);
  assert.strictEqual(normalizeLocalOptions({ bm25AvgLen: Number.NaN }).bm25AvgLen, Number.NaN);
});

test("localExecutor forwards walSegmentMb for native validation", () => {
  // Valid whole MiB survives; absent stays undefined (engine 32 MiB default).
  assert.strictEqual(normalizeLocalOptions({ walSegmentMb: 8 }).walSegmentMb, 8);
  assert.strictEqual(normalizeLocalOptions({}).walSegmentMb, undefined);
  // Invalid values are NOT dropped here — the Rust side rejects them with
  // QQL-VALIDATION-CONFIG instead of silently falling back to the default.
  assert.strictEqual(normalizeLocalOptions({ walSegmentMb: 0 }).walSegmentMb, 0);
  assert.strictEqual(normalizeLocalOptions({ walSegmentMb: -1 }).walSegmentMb, -1);
  assert.strictEqual(normalizeLocalOptions({ walSegmentMb: 1.5 }).walSegmentMb, 1.5);
});

test("localExecutor boolean legacy maps to onDiskPayload", () => {
  assert.deepStrictEqual(normalizeLocalOptions(false), { onDiskPayload: false });
  assert.deepStrictEqual(normalizeLocalOptions(undefined), {});
  assert.deepStrictEqual(normalizeLocalOptions(null), {});
});

test("localExecutor drops non-string model slots", () => {
  const opts = normalizeLocalOptions({ sparseModel: 42, multiModel: null });
  assert.strictEqual(opts.sparseModel, undefined);
  assert.strictEqual(opts.multiModel, undefined);
  assert.strictEqual(opts.model, undefined);
});

test("localExecutor rejects non-object, non-boolean options", () => {
  assert.throws(() => normalizeLocalOptions("nope"), TypeError);
  assert.throws(() => normalizeLocalOptions([]), TypeError);
});

test("standalone options forward edge model slots and embed fields", () => {
  const opts = normalizeStandaloneOptions({
    dataDir: "/data",
    sparseModel: "splade",
    multiModel: "bge-m3",
    imageModel: "clip-vision",
    rerankerModel: "bge-reranker-base",
    embedUrl: "http://localhost:11434/v1/embeddings",
    embedKey: "k",
    embedModel: "nomic-embed-text",
    embedDim: 768,
    bm25K1: 1.5,
    bm25B: 0.6,
    bm25AvgLen: 32,
    onError: "continue",
  });
  assert.strictEqual(opts.sparseModel, "splade");
  assert.strictEqual(opts.multiModel, "bge-m3");
  assert.strictEqual(opts.imageModel, "clip-vision");
  assert.strictEqual(opts.rerankerModel, "bge-reranker-base");
  assert.strictEqual(opts.embedUrl, "http://localhost:11434/v1/embeddings");
  assert.strictEqual(opts.embedKey, "k");
  assert.strictEqual(opts.embedModel, "nomic-embed-text");
  assert.strictEqual(opts.embedDim, 768);
  assert.strictEqual(opts.bm25K1, 1.5);
  assert.strictEqual(opts.bm25B, 0.6);
  assert.strictEqual(opts.bm25AvgLen, 32);
  assert.strictEqual(opts.onError, "continue");
  assert.strictEqual(opts.dataDir, "/data");
});

test("standalone options apply defaults", () => {
  const opts = normalizeStandaloneOptions({});
  assert.strictEqual(opts.dataDir, "./qdrant_data");
  assert.strictEqual(opts.onDiskPayload, true);
  assert.strictEqual(opts.model, undefined);
  assert.strictEqual(opts.embedUrl, undefined);
  assert.strictEqual(opts.bm25K1, undefined);
  assert.strictEqual(opts.bm25B, undefined);
  assert.strictEqual(opts.bm25AvgLen, undefined);
});

test("standalone options forward params for prepared statements", () => {
  // Regression: normalizeStandaloneOptions used to strip `params`, so one-shot
  // execute()/executeStmt() silently ignored :name / ? bindings.
  const named = normalizeStandaloneOptions({ params: { status: "active" } });
  assert.deepStrictEqual(named.params, { status: "active" });
  const positional = normalizeStandaloneOptions({ params: [1, 2] });
  assert.deepStrictEqual(positional.params, [1, 2]);
  const scoped = normalizeStandaloneOptions({ params: [{ a: 1 }, { b: 2 }] });
  assert.deepStrictEqual(scoped.params, [{ a: 1 }, { b: 2 }]);
  assert.strictEqual(normalizeStandaloneOptions({}).params, undefined);
  assert.throws(
    () => normalizeStandaloneOptions({ params: "nope" }),
    /params must be an object/,
  );
});

test("standalone options undefined returns undefined", () => {
  assert.strictEqual(normalizeStandaloneOptions(undefined), undefined);
  assert.strictEqual(normalizeStandaloneOptions(null), undefined);
  assert.throws(() => normalizeStandaloneOptions("x"), TypeError);
});
