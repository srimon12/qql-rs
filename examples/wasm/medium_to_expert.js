/**
 * Medium → Expert (WASM) — multi-tenant gateway.
 */
import init, { isValid, explain, analyze, Stmt, Client } from '../../demo/pkg/qql_wasm.js';

const USERS = {
  alice: { tenant: 'edge-a', role: 'admin' },
  bob: { tenant: 'edge-a', role: 'viewer' },
};

function secure(query, user) {
  const ctx = USERS[user];
  const stmt = new Stmt(query);
  stmt.injectFilter('tenant_id', '=', ctx.tenant);
  stmt.shardKey = ctx.tenant;
  if (ctx.role === 'viewer') stmt.injectFilter('status', '=', 'published');
  return stmt;
}

await init();

const raw = `
  QUERY TEXT 'wasm edge retrieval'
  FROM edge_docs
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  LIMIT 10
`;

console.log('isValid:', isValid(raw));
console.log('explain:\n' + explain(raw));

const secured = secure(raw, 'bob');
console.log('secured shardKey:', secured.shardKey);

const literal = new Stmt(`
  QUERY TEXT 'wasm edge retrieval'
  FROM edge_docs
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  SHARD 'edge-a'
  LIMIT 10
`);
console.log('SHARD in QQL →', literal.shardKey);

console.log('analyze routes:', analyze(raw).routes?.length ?? 0);

const live =
  typeof location !== 'undefined' && new URLSearchParams(location.search).has('live');
if (live) {
  const client = new Client('http://localhost:6333', null);
  client.setHttpEmbedder('http://localhost:11434/v1/embeddings', 'all-minilm:l6-v2', 384, null);
  try {
    console.log(await client.executeStmt(secured));
  } catch (e) {
    console.warn(e);
  }
} else {
  console.log('Offline complete. Append ?live for execute.');
}
