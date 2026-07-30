/**
 * Basic → Medium (Node.js / @veristamp/nqql)
 *
 * Offline walkthrough: parse, isValid, explain, compileQuery,
 * injectFilter, injectShardKey, hybrid shorthand.
 */
import nqql from '../../crates/nqql/index.js';

const {
  parse,
  isValid,
  explain,
  compileQuery,
  injectFilter,
  injectShardKey,
  version,
} = nqql;

console.log(`nqql ${version ?? nqql.__version__ ?? '?'}\n`);

const q = "QUERY TEXT 'machine learning' FROM papers USING dense LIMIT 5";

// 1. Validate & parse
console.log('1. isValid:', isValid(q));
const [stmt] = parse(q);
console.log(
  '   Stmt methods:',
  Object.getOwnPropertyNames(Object.getPrototypeOf(stmt))
    .filter((n) => n !== 'constructor')
    .join(', '),
);
console.log();

// 2. Explain
console.log('2. explain()');
console.log(explain(q));
console.log();

// 3. Compile to REST route
const route = compileQuery(q);
console.log('3. compileQuery()');
console.log(`   ${route.stmt_type}  ${route.method} ${route.path}`);
console.log();

// 4. injectFilter (free function → AST object)
const securedAst = injectFilter(q, 'org_id', '=', 'acme-corp');
console.log('4. injectFilter(org_id = acme-corp)');
console.log('   keys:', Object.keys(securedAst));
console.log();

// 5. Stmt.injectFilter + injectShardKey (in-place)
const [s] = parse(q);
s.injectFilter('tenant_id', '=', 'acme-corp');
s.injectShardKey('acme-corp');
console.log('5. Stmt.injectFilter + injectShardKey');
console.log('   shardKey =', s.shardKey);
console.log();

// 6. Hybrid shorthand
const hybrid = `
  QUERY TEXT 'transformers attention'
  FROM papers
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  LIMIT 5
`;
console.log('6. hybrid shorthand isValid:', isValid(hybrid));
console.log(explain(hybrid));
