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
  });
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
  assert.strictEqual(opts.onError, "continue");
  assert.strictEqual(opts.dataDir, "/data");
});

test("standalone options apply defaults", () => {
  const opts = normalizeStandaloneOptions({});
  assert.strictEqual(opts.dataDir, "./qdrant_data");
  assert.strictEqual(opts.onDiskPayload, true);
  assert.strictEqual(opts.model, undefined);
  assert.strictEqual(opts.embedUrl, undefined);
});

test("standalone options undefined returns undefined", () => {
  assert.strictEqual(normalizeStandaloneOptions(undefined), undefined);
  assert.strictEqual(normalizeStandaloneOptions(null), undefined);
  assert.throws(() => normalizeStandaloneOptions("x"), TypeError);
});
