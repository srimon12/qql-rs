// Benchmark nqql parse across query types.
const nqql = require('../target/release/nqql.node');

const QUERIES = [
  ['Simple', "QUERY 'search' FROM docs LIMIT 10"],
  ['Hybrid', "QUERY 'search' FROM docs LIMIT 10 USING HYBRID"],
  ['Full', "QUERY 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128, exact = true)"],
  ['CTE_Prefetch', "WITH a AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech'), b AS (QUERY 'search' USING sparse LIMIT 100)\nQUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8, b SCORE THRESHOLD 0.5) FUSION RRF"],
  ['CreateCollection', "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"],
  ['Insert', "INSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"],
  ['DeleteWhere', "DELETE FROM docs WHERE category = 'archived'"],
  ['OrderBy', "QUERY ORDER BY created_at DESC FROM docs LIMIT 20 WHERE status = 'active'"],
  ['WithPayload', "QUERY 'search' FROM docs LIMIT 10 WITH PAYLOAD (include = ['title', 'body']) WITH VECTORS ('dense')"],
];

function bench(name, q, iterations) {
  // warmup
  for (let i = 0; i < 1000; i++) nqql.parse(q);

  const start = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) nqql.parse(q);
  const elapsed = Number(process.hrtime.bigint() - start);

  const nsPerOp = elapsed / iterations;
  const opsPerSec = (iterations / elapsed) * 1e9;
  return { nsPerOp, opsPerSec };
}

const iterations = 100_000;
console.log(`Node.js nqql  |  ${iterations} iterations each\n`);
console.log(`${'Query'.padEnd(20)} ${'ns/op'.padStart(10)} ${'ops/s'.padStart(12)}`);
console.log('-'.repeat(46));

for (const [name, q] of QUERIES) {
  const { nsPerOp, opsPerSec } = bench(name, q, iterations);
  console.log(`${name.padEnd(20)} ${nsPerOp.toFixed(0).padStart(10)} ${opsPerSec.toFixed(0).padStart(12)}`);
}
