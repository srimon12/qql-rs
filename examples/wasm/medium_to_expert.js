/**
 * Medium → Expert (WASM / qql-wasm)
 *
 * Multi-tenant browser gateway + optional live Client.execute.
 * Uses Stmt inject APIs so tenant isolation never relies on string concat.
 */
import init, {
  isValid,
  explain,
  analyze,
  compile,
  Stmt,
  Client,
} from '../../demo/pkg/qql_wasm.js';

const USERS = {
  alice: { tenant: 'edge-a', role: 'admin' },
  bob: { tenant: 'edge-a', role: 'viewer' },
};

function secure(query, user) {
  const ctx = USERS[user];
  const stmt = new Stmt(query);
  stmt.injectFilter('tenant_id', '=', ctx.tenant);
  stmt.injectShardKey(ctx.tenant);
  if (ctx.role === 'viewer') {
    stmt.injectFilter('status', '=', 'published');
  }
  return stmt;
}

await init();

const raw = `
  QUERY TEXT 'wasm edge retrieval'
  FROM edge_docs
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  LIMIT 10
`;

console.log('isValid(hybrid):', isValid(raw));
console.log('explain:\n' + explain(raw));

const secured = secure(raw, 'bob');
console.log('secured shardKey:', secured.shardKey);
console.log('secured compileRoute:', secured.compileRoute());

const analysis = analyze(raw);
console.log('analyze routes:', analysis.routes?.length ?? 0, analysis.valid);

// Optional live execute — only if you have Qdrant + CORS configured
const live = typeof location !== 'undefined' && new URLSearchParams(location.search).has('live');
if (live) {
  const client = new Client('http://localhost:6333', null);
  client.setHttpEmbedder(
    'http://localhost:11434/v1/embeddings',
    'nomic-embed-text',
    768,
    null,
  );
  try {
    const report = await client.executeStmt(secured);
    console.log('live report:', report);
  } catch (e) {
    console.warn('live execute failed:', e);
  }
} else {
  console.log('Offline complete. Append ?live to the URL to hit Qdrant from the browser.');
}
