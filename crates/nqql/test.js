const nqql = require("./index.js");
const assert = require("assert");

// Test single parse — returns an array of Stmt objects
const query = "QUERY 'hello' FROM docs LIMIT 10";
const results = nqql.parse(query);
assert(Array.isArray(results), "parse should return an array");
assert.strictEqual(results.length, 1);
const r0 = results[0];
assert(r0 instanceof nqql.Stmt);
const r0Object = r0.toObject();
assert(r0Object.Query !== undefined);
assert.strictEqual(r0Object.Query.collection.Explicit, "docs");
assert.strictEqual(r0Object.Query.expression.Nearest.input.Text.text, "hello");

// Test multi-statement parse
const multiResults = nqql.parse(
  "QUERY 'test' FROM users LIMIT 5; CREATE COLLECTION items"
);
assert(Array.isArray(multiResults));
assert.strictEqual(multiResults.length, 2);
assert.strictEqual(multiResults[0].toObject().Query.collection.Explicit, "users");
assert(multiResults[1].toObject().CreateCollection !== undefined);

// Test tokenize
const tokens = nqql.tokenize("QUERY 'test' FROM docs");
assert(Array.isArray(tokens));
assert(tokens.length > 0);
assert.strictEqual(tokens[0].text, "QUERY");
assert.strictEqual(tokens[0].pos, 0);
assert.strictEqual(tokens[0].end, 5);
assert.strictEqual(tokens[0].len, 5);

// Test Stmt compileRoute
const stmtRoute = r0.compileRoute();
assert.strictEqual(stmtRoute.stmt_type, "query");
assert.strictEqual(stmtRoute.method, "POST");
assert.strictEqual(stmtRoute.path, "/collections/docs/points/query");
assert(stmtRoute.payload && typeof stmtRoute.payload === "object");

// Test route compilation contract
const route = nqql.compileQuery(query);
assert.strictEqual(route.stmt_type, "query");
assert.strictEqual(route.method, "POST");
assert.strictEqual(route.path, "/collections/docs/points/query");
assert(route.payload && typeof route.payload === "object");

// Test DELETE PAYLOAD compilation contract
const deletePayloadRoute = nqql.compileQuery(
  "DELETE PAYLOAD draft, temp_token FROM docs WHERE status = 'archived' SHARD 'tenant_1'"
);
assert.strictEqual(deletePayloadRoute.stmt_type, "delete_payload");
assert.strictEqual(deletePayloadRoute.method, "POST");
assert.strictEqual(deletePayloadRoute.path, "/collections/docs/points/payload/delete");
assert.deepStrictEqual(deletePayloadRoute.payload.keys, ["draft", "temp_token"]);

// Test COUNT WITH (exact = true)
const countRoute = nqql.compileQuery("COUNT FROM docs WHERE active = true WITH (exact = true)");
assert.strictEqual(countRoute.stmt_type, "count");
assert.strictEqual(countRoute.payload.exact, true);

// Test GROUP BY OFFSET effective limit
const groupRoute = nqql.compileQuery("QUERY 'test' FROM docs GROUP BY category LIMIT 10 OFFSET 5");
assert.strictEqual(groupRoute.stmt_type, "query_groups");
assert.strictEqual(groupRoute.payload.limit, 15);

// Test explain
const plan = nqql.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(plan.includes("Statement: QUERY"));
assert(plan.includes("Collection: docs"));

// Test Client with default settings
const client = new nqql.Client({ url: "http://localhost:6333", useGrpc: false });
const clientPlan = client.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(clientPlan.includes("Collection: docs"));

// Test Client with first-class HttpEmbedder object
const embedder = new nqql.HttpEmbedder({
  endpoint: "http://localhost:11434/v1/embeddings",
  model: "nomic-embed-text",
  dimension: 768,
  apiKey: "embed-key",
});
const clientWithEmbedder = new nqql.Client({
  url: "http://localhost:6333",
  apiKey: "test-key",
  embedder: embedder,
});
const customPlan = clientWithEmbedder.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(customPlan.includes("Statement: QUERY"));
assert.throws(
  () => new nqql.HttpEmbedder({ endpoint: "http://localhost", model: "test" }),
  /dimension must be a positive integer/,
);

// Qdrant 1.19 route affinity is accepted at construction and readable.
const affinityClient = new nqql.Client({
  url: "http://localhost:6333",
  routeAffinity: "session-acme-42",
});
assert.strictEqual(affinityClient.routeAffinity, "session-acme-42");
const affinityPlan = affinityClient.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(affinityPlan.includes("Collection: docs"));
const unsetClient = new nqql.Client({ url: "http://localhost:6333", routeAffinity: "" });
assert.strictEqual(unsetClient.routeAffinity, null);
assert.strictEqual(new nqql.Client({ url: "http://localhost:6333" }).routeAffinity, null);

// RT-05: HttpEmbedder accepts rerank fields (camelCase convention)
const embedderRerank = new nqql.HttpEmbedder({
  endpoint: "http://localhost:11434/v1/embeddings",
  model: "nomic-embed-text",
  dimension: 768,
  rerankEndpoint: "http://localhost:11434/rerank",
  rerankModel: "test-reranker",
  rerankApiKey: "rk-key",
});
assert.strictEqual(embedderRerank.rerankEndpoint, "http://localhost:11434/rerank");
assert.strictEqual(embedderRerank.rerankModel, "test-reranker");
assert.strictEqual(embedderRerank.rerankApiKey, "rk-key");
const clientRerank = new nqql.Client({
  url: "http://localhost:6333",
  embedder: embedderRerank,
});
const rerankPlan = clientRerank.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(rerankPlan.includes("Collection: docs"));

// RT-07: HttpEmbedder forwards multi/image embedder fields (camelCase + snake_case)
const embedderMultiImage = new nqql.HttpEmbedder({
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
});
assert.strictEqual(embedderMultiImage.multiEndpoint, "http://localhost:11434/multi");
assert.strictEqual(embedderMultiImage.multiApiKey, "mk");
assert.strictEqual(embedderMultiImage.multiModel, "colbert");
assert.strictEqual(embedderMultiImage.multiDimension, 256);
assert.strictEqual(embedderMultiImage.imageEndpoint, "http://localhost:11434/image");
assert.strictEqual(embedderMultiImage.imageApiKey, "ik");
assert.strictEqual(embedderMultiImage.imageModel, "clip-vit-b32");
assert.strictEqual(embedderMultiImage.imageDimension, 512);
assert.strictEqual(embedderMultiImage.rerankEndpoint, ""); // default empty

const embedderMultiImageSnake = new nqql.HttpEmbedder({
  endpoint: "http://localhost:11434/v1/embeddings",
  model: "nomic-embed-text",
  dimension: 768,
  multi_endpoint: "http://localhost:11434/multi",
  multi_api_key: "mk",
  multi_model: "colbert",
  multi_dimension: 256,
  image_endpoint: "http://localhost:11434/image",
  image_api_key: "ik",
  image_model: "clip-vit-b32",
  image_dimension: 512,
});
assert.strictEqual(embedderMultiImageSnake.multiEndpoint, "http://localhost:11434/multi");
assert.strictEqual(embedderMultiImageSnake.multiApiKey, "mk");
assert.strictEqual(embedderMultiImageSnake.multiModel, "colbert");
assert.strictEqual(embedderMultiImageSnake.multiDimension, 256);
assert.strictEqual(embedderMultiImageSnake.imageEndpoint, "http://localhost:11434/image");
assert.strictEqual(embedderMultiImageSnake.imageApiKey, "ik");
assert.strictEqual(embedderMultiImageSnake.imageModel, "clip-vit-b32");
assert.strictEqual(embedderMultiImageSnake.imageDimension, 512);

// Multi/image dimensions must be positive integers when supplied.
assert.throws(
  () =>
    new nqql.HttpEmbedder({
      endpoint: "http://x",
      model: "m",
      dimension: 4,
      multiDimension: "256",
    }),
  /multiDimension must be a positive integer/,
);
assert.throws(
  () =>
    new nqql.HttpEmbedder({
      endpoint: "http://x",
      model: "m",
      dimension: 4,
      imageDimension: 0,
    }),
  /imageDimension must be a positive integer/,
);
assert.throws(
  () =>
    new nqql.HttpEmbedder({
      endpoint: "http://x",
      model: "m",
      dimension: 4,
      multiApiKey: 123,
    }),
  /multiApiKey must be a string/,
);

// Client construction accepts the full multi/image embedder surface.
const clientMultiImage = new nqql.Client({
  url: "http://localhost:6333",
  embedder: embedderMultiImage,
});
const multiImagePlan = clientMultiImage.explain("QUERY 'hello' FROM docs LIMIT 10");
assert(multiImagePlan.includes("Collection: docs"));

// Invalid filter operators must never silently become equality.
assert.throws(
  () => nqql.injectFilter(query, "tenant_id", "contains", "acme"),
  /unsupported comparison operator/,
);
assert.throws(
  () => r0.injectFilter("tenant_id", "contains", "acme"),
  /unsupported comparison operator/,
);

// Test error handling — structured QqlError fields
try {
  nqql.parse("invalid syntax");
  assert.fail("Should have thrown an error");
} catch (e) {
  assert(e.message.includes("expected a QQL statement keyword"));
  assert.strictEqual(e.code, "QQL-PARSE-STATEMENT");
  assert.strictEqual(e.kind, "Parse");
  assert.deepStrictEqual(e.span, { start: 0, end: 7 });
}

// Parameter binding tests
const qNamed = "QUERY 'shoes' FROM products WHERE category = :cat AND price < :max_p";
const boundNamed = nqql.bind(qNamed, { cat: "sneakers", max_p: 100 });
assert.strictEqual(
  boundNamed,
  "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND price < 100",
);
const boundNamedFn = nqql.bind(qNamed, { cat: "boots", max_p: 50 });
assert.strictEqual(
  boundNamedFn,
  "QUERY 'shoes' FROM products WHERE category = 'boots' AND price < 50",
);

const qPos = "QUERY 'shoes' FROM products WHERE category = ? AND in_stock = ?";
const boundPos = nqql.bind(qPos, ["sneakers", true]);
assert.strictEqual(
  boundPos,
  "QUERY 'shoes' FROM products WHERE category = 'sneakers' AND in_stock = true",
);
const boundPosFn = nqql.bind(qPos, ["boots", false]);
assert.strictEqual(
  boundPosFn,
  "QUERY 'shoes' FROM products WHERE category = 'boots' AND in_stock = false",
);

// Boolean ordering & dollar identifier tests
assert.strictEqual(
  nqql.bind("WHERE flag = ? AND count = ?", [true, 1]),
  "WHERE flag = true AND count = 1",
);
assert.strictEqual(
  nqql.bind("WHERE $category = :cat AND $1 = 42", { cat: "boots" }),
  "WHERE $category = 'boots' AND $1 = 42",
);

// Shape validation tests
assert.throws(() => nqql.bind("WHERE x = :x", [1]), /named placeholder|MIXED-STYLE/);
assert.throws(() => nqql.bind("WHERE x = ?", { x: 1 }), /positional placeholder|MIXED-STYLE/);

async function testAsyncErrors() {
  await assert.rejects(
    client.execute("invalid syntax"),
    (error) =>
      error.code === "QQL-PARSE-STATEMENT" &&
      error.kind === "Parse" &&
      error.span.start === 0,
  );
  await assert.rejects(
    client.execute("SHOW COLLECTIONS", { onError: "typo" }),
    /options\.onError must be 'stop' or 'continue'/,
  );
  const report = await client.execute("invalid syntax", {
    onError: "continue",
  });
  assert.strictEqual(report.ok, false);
  assert.strictEqual(report.succeeded, 0);
  assert.strictEqual(report.failed, 1);
  assert.strictEqual(report.results[0].operation, "PARSE");
  await client.close();
}

testAsyncErrors()
  .then(() => console.log("All NAPI tests passed!"))
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
