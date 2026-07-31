/**
 * Medium → Expert (WASM) — multi-tenant gateway.
 *
 * Uses the checked-in Node build of the qql-wasm crate
 * (`crates/qql-wasm/pkg-node`, auto-instantiated — no `init()` needed).
 * Run: node examples/wasm/medium_to_expert.js
 */
const { isValid, explain, analyze, Stmt, Client } = require('../../crates/qql-wasm/pkg-node/qql_wasm.js');

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

async function run() {
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

  const routes = analyze(raw).routes ?? [];
  console.log('analyze routes:', routes.length);

  // Live execution is opt-in: QDRANT_URL must point at a running Qdrant.
  if (process.env.QDRANT_URL) {
    const client = new Client(process.env.QDRANT_URL, null);
    client.setHttpEmbedder('http://localhost:11434/v1/embeddings', 'all-minilm:l6-v2', 384, null);
    try {
      console.log(await client.executeStmt(secured));
    } catch (e) {
      console.warn(e);
    }
  } else {
    console.log('Offline complete. Set QDRANT_URL for live execute.');
  }
}

run().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
