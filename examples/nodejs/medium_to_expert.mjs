/**
 * Medium → Expert (Node.js / @veristamp/nqql)
 *
 * Multi-tenant security gateway with hybrid retrieval strategies.
 * Offline by default; pass --live to hit Qdrant (needs collection).
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
  formula: (q) => `
    WITH candidates AS (
      QUERY TEXT '${q}' FROM research_papers USING dense LIMIT 40
    )
    QUERY FORMULA (score * 2.0) DEFAULTS (score = 0.0)
    FROM research_papers
    PREFETCH (candidates)
    LIMIT 5
  `,
};

function secure(query, user) {
  const ctx = USERS[user];
  const [stmt] = parse(query);
  stmt.injectFilter('tenant_id', '=', ctx.tenant);
  stmt.injectShardKey(ctx.tenant);
  if (ctx.role === 'viewer') {
    stmt.injectFilter('status', '=', 'published');
  }
  return stmt;
}

const live = process.argv.includes('--live');
const user = 'bob';
const strategy = 'hybrid';
const question = 'transformers attention mechanism';

console.log(`nqql ${version ?? nqql.__version__ ?? '?'}`);
console.log(`user=${user} tenant=${USERS[user].tenant} role=${USERS[user].role}\n`);

const raw = STRATEGIES[strategy](question);
console.log('── raw QQL ──');
console.log(raw.trim());
console.log();

const secured = secure(raw, user);
console.log('── secured ──');
console.log('  shardKey =', secured.shardKey);
console.log('  toJSON   =', secured.toJSON().slice(0, 200) + '…');
console.log();

console.log('── strategy inventory ──');
for (const [name, fn] of Object.entries(STRATEGIES)) {
  const q = fn('demo');
  const s = secure(q, user);
  console.log(`  ${name.padEnd(12)} valid=${isValid(q)}  shard=${s.shardKey}`);
}

console.log('\n── explain (hybrid base plan) ──');
console.log(explain(raw));

if (!live) {
  console.log('\nOffline complete. Re-run with --live to hit Qdrant.');
  process.exit(0);
}

const embedder = new HttpEmbedder({
  endpoint: 'http://localhost:11434/v1/embeddings',
  model: 'nomic-embed-text',
  dimension: 768,
});
const client = new Client({ url: 'http://localhost:6333', embedder });
console.log('\n── live execute ──');
try {
  const report = await client.execute(secured);
  console.log(JSON.stringify(report, null, 2).slice(0, 500));
} catch (e) {
  console.log('  execute failed (expected if collection missing):', e.message ?? e);
}
