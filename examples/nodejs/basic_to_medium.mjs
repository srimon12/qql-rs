/**
 * Basic → Medium (Node.js / @veristamp/nqql)
 * Offline: parse, explain, compile, injectFilter, SHARD / shardKey property.
 */
import nqql from '../../crates/nqql/index.js';

const { parse, isValid, explain, compileQuery, injectFilter, version } = nqql;

console.log(`nqql ${version ?? nqql.__version__ ?? '?'}\n`);

const q = "QUERY TEXT 'machine learning' FROM papers USING dense LIMIT 5";

console.log('1. isValid:', isValid(q));
const [stmt] = parse(q);
console.log('   Stmt methods:', Object.getOwnPropertyNames(Object.getPrototypeOf(stmt)).filter((n) => n !== 'constructor').join(', '));
console.log();

console.log('2. explain()\n' + explain(q) + '\n');

const route = compileQuery(q);
console.log('3. compileQuery()', route.stmt_type, route.method, route.path, '\n');

const securedAst = injectFilter(q, 'org_id', '=', 'acme-corp');
console.log('4. injectFilter keys:', Object.keys(securedAst), '\n');

// Preferred: SHARD in QQL
const [withShard] = parse(
  "QUERY TEXT 'machine learning' FROM papers USING dense SHARD 'acme-corp' LIMIT 5",
);
console.log('5a. SHARD in QQL → shardKey =', withShard.shardKey);

// Host path after parse
const [s] = parse(q);
s.injectFilter('tenant_id', '=', 'acme-corp');
s.shardKey = 'acme-corp';
console.log('5b. stmt.shardKey = … →', s.shardKey);

const hybrid = `
  QUERY TEXT 'transformers attention'
  FROM papers
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  SHARD 'acme-corp'
  LIMIT 5
`;
console.log('\n6. hybrid + SHARD isValid:', isValid(hybrid));
console.log(explain(hybrid));
