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
}

testAsyncErrors()
  .then(() => console.log("All NAPI tests passed!"))
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
