const nqql = require('./index.linux-x64-gnu.node');
const assert = require('assert');

// Test single parse
const query = "QUERY 'hello' FROM docs LIMIT 10";
const ast = nqql.parse(query);

assert(typeof ast === 'object');
assert(ast.Query !== undefined);
assert.strictEqual(ast.Query.collection, "docs");
assert.strictEqual(ast.Query.query_text, "hello");

// Test parse batch
const queries = ["QUERY 'test' FROM users LIMIT 5", "CREATE COLLECTION items"];
const results = nqql.parseBatch(queries);

assert(Array.isArray(results));
assert.strictEqual(results.length, 2);
assert.strictEqual(results[0].Query.collection, "users");
assert(results[1].CreateCollection !== undefined);

// Test tokenize
const tokens = nqql.tokenize("QUERY 'test' FROM docs");
assert(Array.isArray(tokens));
assert(tokens.length > 0);
assert.strictEqual(tokens[0].text, "QUERY");

// Test error handling
try {
    nqql.parse("invalid syntax");
    assert.fail("Should have thrown an error");
} catch (e) {
    assert(e.message.includes("expected a QQL statement keyword"));
}

console.log("All NAPI tests passed!");
