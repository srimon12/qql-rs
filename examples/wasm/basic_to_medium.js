/**
 * Basic → Medium (WASM / qql-wasm)
 *
 * Browser-side: parse, validate, compile, injectFilter, injectShardKey, analyze.
 * No Qdrant required.
 *
 * Serve this directory (or the parent) with any static HTTP server after building:
 *   cd crates/qql-wasm && wasm-pack build --target web --out-dir ../../demo/pkg
 *   # or use the prebuilt demo/pkg
 */
import init, {
  parse,
  isValid,
  compile,
  explain,
  analyze,
  Stmt,
  injectShardKey,
} from '../../demo/pkg/qql_wasm.js';

async function run() {
  await init();

  const query = "QUERY TEXT 'browser search' FROM docs USING dense LIMIT 5";

  console.log('1. isValid:', isValid(query));

  const ast = parse(query);
  console.log('2. parse AST statements:', Array.isArray(ast) ? ast.length : 1);

  console.log('3. explain:\n' + explain(query));

  const route = compile(query);
  console.log('4. compile:', route.stmt_type, route.method, route.path);

  // Stmt object — inject filter + shard key in place
  const stmt = new Stmt(query);
  stmt.injectFilter('tenant_id', '=', 'browser-tenant');
  stmt.injectShardKey('browser-tenant');
  console.log('5. Stmt.injectFilter + injectShardKey → shardKey =', stmt.shardKey);

  // Free-function injectShardKey
  const sharded = injectShardKey(query, 'browser-tenant');
  console.log('6. injectShardKey free function keys:', Object.keys(sharded));

  // One-shot analyze: parse + explain + routes
  const hybrid = `
    QUERY TEXT 'wasm performance'
    FROM edge_docs
    USING HYBRID DENSE dense SPARSE sparse FUSION RRF
    LIMIT 10
  `;
  const report = analyze(hybrid);
  console.log('7. analyze(hybrid):', {
    valid: report.valid,
    statements_count: report.statements_count,
    route: report.route && `${report.route.method} ${report.route.path}`,
  });
}

run().catch((e) => {
  console.error(e);
  throw e;
});
