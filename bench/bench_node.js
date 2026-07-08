const nqql = require('../crates/nqql/index.js');

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

function bench_napi(name, q, iterations) {
    // warmup
    for (let i = 0; i < 100; i++) {
        nqql.parse(q)
    }
    
    let start = process.hrtime.bigint()
    for (let i = 0; i < iterations; i++) {
        nqql.parse(q)
    }
    let end = process.hrtime.bigint()
    let elapsed = Number(end - start)
    return {
        ns_per_op: elapsed / iterations,
        ops_per_sec: (iterations / elapsed) * 1e9
    }
}

function bench_json(name, q, iterations) {
    // warmup
    for (let i = 0; i < 100; i++) {
        nqql.parseFastJson(q)
    }
    
    let start = process.hrtime.bigint()
    for (let i = 0; i < iterations; i++) {
        nqql.parseFastJson(q)
    }
    let end = process.hrtime.bigint()
    let elapsed = Number(end - start)
    return {
        ns_per_op: elapsed / iterations,
        ops_per_sec: (iterations / elapsed) * 1e9
    }
}

const iterations = 100_000;
console.log(`Node.js nqql  |  ${iterations} iterations each\n`);
console.log(`${'Query'.padEnd(20)} | ${'NAPI parse()'.padStart(15)} | ${'parseFastJson()'.padStart(15)}`);
console.log('-'.repeat(60));

for (const [name, q] of QUERIES) {
  const n_napi = bench_napi(name, q, iterations);
  const n_json = bench_json(name, q, iterations);
  console.log(`${name.padEnd(20)} | ${n_napi.ops_per_sec.toFixed(0).padStart(15)} | ${n_json.ops_per_sec.toFixed(0).padStart(15)}`);
}
