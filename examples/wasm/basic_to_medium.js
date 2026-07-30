/**
 * Basic → Medium (WASM) — parse, compile, injectFilter, SHARD / shardKey.
 */
import init, {
  parse,
  isValid,
  compile,
  explain,
  analyze,
  Stmt,
} from '../../demo/pkg/qql_wasm.js';

async function run() {
  await init();

  const query = "QUERY TEXT 'browser search' FROM docs USING dense LIMIT 5";
  console.log('1. isValid:', isValid(query));
  console.log('2. parse:', parse(query));
  console.log('3. explain:\n' + explain(query));
  console.log('4. compile:', compile(query));

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
  console.log('6. analyze(hybrid):', analyze(hybrid));
}

run().catch((e) => {
  console.error(e);
  throw e;
});
