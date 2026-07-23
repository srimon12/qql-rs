const nqql = require("./index.node");
const assert = require("assert");

// Test single parse
const query = "QUERY 'hello' FROM docs LIMIT 10";
const ast = nqql.parse(query);

assert(typeof ast === "object");
const obj = ast.toObject();
assert(obj.Query !== undefined);
assert.strictEqual(obj.Query.collection.Explicit, "docs");
assert.strictEqual(
  obj.Query.expression.Nearest.input.Text.text,
  "hello"
);

// Test parse batch
const queries = ["QUERY 'test' FROM users LIMIT 5", "CREATE COLLECTION items"];
const results = nqql.parseBatch(queries);

assert(Array.isArray(results));
assert.strictEqual(results.length, 2);
const r0 = results[0].toObject();
const r1 = results[1].toObject();
assert.strictEqual(r0.Query.collection.Explicit, "users");
assert(r1.CreateCollection !== undefined);

// Test tokenize
const tokens = nqql.tokenize("QUERY 'test' FROM docs");
assert(Array.isArray(tokens));
assert(tokens.length > 0);
assert.strictEqual(tokens[0].text, "QUERY");

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
const customPlan = clientWithEmbedder.explain(
  "QUERY 'hello' FROM docs LIMIT 10"
);
assert(customPlan.includes("Statement: QUERY"));

// Test error handling
try {
  nqql.parse("invalid syntax");
  assert.fail("Should have thrown an error");
} catch (e) {
  assert(e.message.includes("expected a QQL statement keyword"));
}

console.log("All NAPI tests passed!");
