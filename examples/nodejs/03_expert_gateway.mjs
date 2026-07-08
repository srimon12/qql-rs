// 03 Expert: Multi-tenant query gateway using injectFilter as auth middleware.
import { injectFilter } from 'nqql';

const USERS = {
  alice:   { tenant: 'acme',  role: 'admin' },
  bob:     { tenant: 'acme',  role: 'viewer' },
  charlie: { tenant: 'globex', role: 'viewer' },
};

function enforce(user, query) {
  const ctx = USERS[user];
  let safe = injectFilter(query, 'tenant_id', '=', ctx.tenant);
  if (ctx.role === 'viewer') {
    safe = injectFilter(safe, 'status', '!=', 'confidential');
  }
  return safe;
}

const requests = [
  ['alice',   "QUERY 'sales data' FROM analytics LIMIT 10"],
  ['bob',     "QUERY 'sales data' FROM analytics LIMIT 10"],
  ['charlie', "QUERY 'engineering docs' FROM docs LIMIT 5"],
];

console.log('=== QQL Query Gateway ===');
for (const [user, raw] of requests) {
  const safe = enforce(user, raw);
  console.log(`\n  user=${user.padEnd(8)} role=${USERS[user].role.padEnd(7)}`);
  console.log(`  raw:  ${raw}`);
  console.log(`  safe: ${safe.substring(0, 130)}...`);
}
