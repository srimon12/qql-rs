"use strict";

/**
 * Wrapper-only tests for nqql option normalization.
 * No native binding required — run via `npm run test:options`.
 */

const { test } = require("node:test");
const assert = require("node:assert");
const { normalizeClientOptions } = require("./options.js");

test("client options forward apiKey/useGrpc aliases", () => {
  const opts = normalizeClientOptions({ url: "http://x", api_key: "k", use_grpc: true });
  assert.strictEqual(opts.apiKey, "k");
  assert.strictEqual(opts.useGrpc, true);
  assert.strictEqual(opts.url, "http://x");
});

test("client options forward routeAffinity (camelCase wins over snake_case)", () => {
  const camel = normalizeClientOptions({ routeAffinity: "session-42" });
  assert.strictEqual(camel.routeAffinity, "session-42");
  const snake = normalizeClientOptions({ route_affinity: "session-42" });
  assert.strictEqual(snake.routeAffinity, "session-42");
  const both = normalizeClientOptions({
    routeAffinity: "camel",
    route_affinity: "snake",
  });
  assert.strictEqual(both.routeAffinity, "camel");
  const none = normalizeClientOptions({ url: "http://x" });
  assert.strictEqual(none.routeAffinity, undefined);
});

test("client options embedder forwards dense fields", () => {
  const opts = normalizeClientOptions({
    embedder: {
      endpoint: "http://localhost:11434/v1/embeddings",
      apiKey: "sk",
      model: "nomic-embed-text",
      dimension: 768,
    },
  });
  assert.deepStrictEqual(opts.embedder, {
    endpoint: "http://localhost:11434/v1/embeddings",
    apiKey: "sk",
    model: "nomic-embed-text",
    dimension: 768,
    multiEndpoint: undefined,
    multiApiKey: undefined,
    multiModel: undefined,
    multiDimension: undefined,
    imageEndpoint: undefined,
    imageApiKey: undefined,
    imageModel: undefined,
    imageDimension: undefined,
    rerankEndpoint: undefined,
    rerankApiKey: undefined,
    rerankModel: undefined,
  });
});

test("client options embedder forwards multi/image/rerank fields (camelCase)", () => {
  const opts = normalizeClientOptions({
    embedder: {
      endpoint: "http://localhost:11434/v1/embeddings",
      model: "nomic-embed-text",
      dimension: 768,
      multiEndpoint: "http://localhost:11434/multi",
      multiApiKey: "mk",
      multiModel: "colbert",
      multiDimension: 256,
      imageEndpoint: "http://localhost:11434/image",
      imageApiKey: "ik",
      imageModel: "clip-vit-b32",
      imageDimension: 512,
      rerankEndpoint: "http://localhost:11434/rerank",
      rerankApiKey: "rk",
      rerankModel: "bge-reranker-base",
    },
  });
  assert.strictEqual(opts.embedder.multiEndpoint, "http://localhost:11434/multi");
  assert.strictEqual(opts.embedder.multiApiKey, "mk");
  assert.strictEqual(opts.embedder.multiModel, "colbert");
  assert.strictEqual(opts.embedder.multiDimension, 256);
  assert.strictEqual(opts.embedder.imageEndpoint, "http://localhost:11434/image");
  assert.strictEqual(opts.embedder.imageApiKey, "ik");
  assert.strictEqual(opts.embedder.imageModel, "clip-vit-b32");
  assert.strictEqual(opts.embedder.imageDimension, 512);
  assert.strictEqual(opts.embedder.rerankEndpoint, "http://localhost:11434/rerank");
  assert.strictEqual(opts.embedder.rerankApiKey, "rk");
  assert.strictEqual(opts.embedder.rerankModel, "bge-reranker-base");
});

test("client options embedder accepts snake_case aliases", () => {
  const opts = normalizeClientOptions({
    embedder: {
      endpoint: "http://x",
      api_key: "sk",
      model: "m",
      dimension: 4,
      multi_endpoint: "http://x/multi",
      multi_api_key: "mk",
      multi_model: "colbert",
      multi_dimension: 256,
      image_endpoint: "http://x/image",
      image_api_key: "ik",
      image_model: "clip",
      image_dimension: 512,
      rerank_endpoint: "http://x/rerank",
      rerank_api_key: "rk",
      rerank_model: "reranker",
    },
  });
  assert.strictEqual(opts.embedder.apiKey, "sk");
  assert.strictEqual(opts.embedder.multiEndpoint, "http://x/multi");
  assert.strictEqual(opts.embedder.multiApiKey, "mk");
  assert.strictEqual(opts.embedder.multiModel, "colbert");
  assert.strictEqual(opts.embedder.multiDimension, 256);
  assert.strictEqual(opts.embedder.imageEndpoint, "http://x/image");
  assert.strictEqual(opts.embedder.imageApiKey, "ik");
  assert.strictEqual(opts.embedder.imageModel, "clip");
  assert.strictEqual(opts.embedder.imageDimension, 512);
  assert.strictEqual(opts.embedder.rerankEndpoint, "http://x/rerank");
  assert.strictEqual(opts.embedder.rerankApiKey, "rk");
  assert.strictEqual(opts.embedder.rerankModel, "reranker");
});

test("client options without embedder leaves the slot undefined", () => {
  const opts = normalizeClientOptions({ url: "http://x" });
  assert.strictEqual(opts.embedder, undefined);
});

test("client options undefined/null returns undefined", () => {
  assert.strictEqual(normalizeClientOptions(undefined), undefined);
  assert.strictEqual(normalizeClientOptions(null), undefined);
  assert.throws(() => normalizeClientOptions("x"), TypeError);
  assert.throws(() => normalizeClientOptions([]), TypeError);
});
