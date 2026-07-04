// 03 Expert: Multi-tenant query gateway — runs entirely in the browser.
async function main() {
  const qql = await import('qql-wasm');

  const USERS = {
    alice:   { tenant: 'acme',  role: 'admin' },
    bob:     { tenant: 'acme',  role: 'viewer' },
    charlie: { tenant: 'globex', role: 'viewer' },
  };

  function enforce(user, query) {
    const ctx = USERS[user];
    let safe = qql.inject_filter(query, 'tenant_id', '=', `{"str": "${ctx.tenant}"}`);
    if (ctx.role === 'viewer') {
      safe = qql.inject_filter(safe, 'status', '!=', '{"str": "confidential"}');
    }
    return safe;
  }

  const requests = [
    ['alice',   "QUERY 'sales data' FROM analytics LIMIT 10"],
    ['bob',     "QUERY 'sales data' FROM analytics LIMIT 10"],
    ['charlie', "QUERY 'engineering docs' FROM docs LIMIT 5"],
  ];

  console.log('=== QQL Query Gateway (Browser) ===');
  for (const [user, raw] of requests) {
    const safe = enforce(user, raw);
    console.log(`\n  user=${user.padEnd(8)} role=${USERS[user].role.padEnd(7)}`);
    console.log(`  raw:  ${raw}`);
    console.log(`  safe: ${safe.substring(0, 130)}...`);
  }
}
main().catch(console.error);
