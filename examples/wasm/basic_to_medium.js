/**
 * Basic → Medium (WASM) — parse, compile, injectFilter, SHARD / shardKey.
 *
 * Uses the checked-in Node build of the qql-wasm crate
 * (`crates/qql-wasm/pkg-node`, auto-instantiated — no `init()` needed).
 * Run: node examples/wasm/basic_to_medium.js
 */
const {
  parse,
  isValid,
  compile,
  explain,
  analyze,
  Stmt,
} = require('../../crates/qql-wasm/pkg-node/qql_wasm.js');

const query = "QUERY TEXT 'browser search' FROM docs USING dense LIMIT 5";
console.log('1. isValid:', isValid(query));
console.log('2. parse:', JSON.stringify(parse(query)));
console.log('3. explain:\n' + explain(query));
console.log('4. compile:', JSON.stringify(compile(query)));

// SHARD in QQL
const sharded = new Stmt(
  "QUERY TEXT 'browser search' FROM docs USING dense SHARD 'browser-tenant' LIMIT 5",
);
console.log('5a. SHARD in QQL → shardKey =', sharded.shardKey);

// Host property after parse
const stmt = new Stmt(query);
stmt.injectFilter('tenant_id', '=', 'browser-tenant');
stmt.shardKey = 'browser-tenant';
console.log('5b. stmt.shardKey =', stmt.shardKey);

const hybrid = `
  QUERY TEXT 'wasm performance'
  FROM edge_docs
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  SHARD 'browser-tenant'
  LIMIT 10
`;
console.log('6. analyze(hybrid):', JSON.stringify(analyze(hybrid)));
