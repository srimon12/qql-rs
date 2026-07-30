/**
 * Medium → Expert (Node.js) — multi-tenant gateway.
 * injectFilter always; SHARD via QQL or stmt.shardKey.
 */
import nqql from '../../crates/nqql/index.js';

const { parse, isValid, explain, Client, HttpEmbedder, version } = nqql;

const USERS = {
  alice: { tenant: 'lab-alpha', role: 'admin' },
  bob: { tenant: 'lab-alpha', role: 'viewer' },
  charlie: { tenant: 'lab-beta', role: 'viewer' },
};

const STRATEGIES = {
  hybrid: (q) => `
    QUERY TEXT '${q}'
    FROM research_papers
    USING HYBRID DENSE dense SPARSE sparse FUSION RRF
    LIMIT 5
  `,
  multistage: (q) => `
    WITH
      dense AS (QUERY TEXT '${q}' FROM research_papers USING dense LIMIT 100),
      sparse AS (QUERY TEXT '${q}' FROM research_papers USING sparse LIMIT 100)
    QUERY FUSION RRF FROM research_papers
    PREFETCH (dense, sparse)
    LIMIT 5
  `,
};

function secure(query, user) {
  const ctx = USERS[user];
  const [stmt] = parse(query);
  stmt.injectFilter('tenant_id', '=', ctx.tenant);
  stmt.shardKey = ctx.tenant;
  if (ctx.role === 'viewer') stmt.injectFilter('status', '=', 'published');
  return stmt;
}

const live = process.argv.includes('--live');
const user = 'bob';
const raw = STRATEGIES.hybrid('transformers attention mechanism');

console.log(`nqql ${version ?? '?'}`);
console.log(`user=${user} tenant=${USERS[user].tenant}\n`);
console.log('── raw ──\n' + raw.trim() + '\n');

const secured = secure(raw, user);
console.log('── secured ──');
console.log('  shardKey =', secured.shardKey);
console.log('  toJSON   =', secured.toJSON().slice(0, 180) + '…\n');

// Template with SHARD in the language (preferred when tenant known at write time)
const literal = `
  QUERY TEXT 'transformers'
  FROM research_papers
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  SHARD '${USERS[user].tenant}'
  LIMIT 5
`;
const [lit] = parse(literal);
console.log('── SHARD in QQL ── shardKey =', lit.shardKey, 'valid=', isValid(literal));
console.log('\n── explain ──\n' + explain(raw));

if (!live) {
  console.log('\nOffline complete.');
  process.exit(0);
}

const client = new Client({
  url: 'http://localhost:6333',
  embedder: new HttpEmbedder({
    endpoint: 'http://localhost:11434/v1/embeddings',
    model: 'all-minilm:l6-v2',
    dimension: 384,
  }),
});
try {
  console.log(JSON.stringify(await client.execute(secured), null, 2).slice(0, 500));
} catch (e) {
  console.log('live failed:', e.message ?? e);
}
