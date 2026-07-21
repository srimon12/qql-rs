const nqql = require('../crates/nqql/index.js');

const QUERIES = [
  ['Simple', "QUERY 'search' FROM docs LIMIT 10"],
  ['Hybrid', "QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10"],
  ['Full', "QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2"],
  ['CTE_Prefetch', "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10"],
  ['CreateCollection', "CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)"],
  ['Upsert', "UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}"],
  ['DeleteWhere', "DELETE FROM docs WHERE category = 'archived'"],
  ['OrderBy', "QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20"],
  ['WithPayload', "QUERY 'search' FROM docs WITH PAYLOAD INCLUDE (title, body) WITH VECTOR (dense) LIMIT 10"],
];

function bench_napi_parse(name, q, iterations) {
    for (let i = 0; i < 100; i++) {
        nqql.parse(q)
    }
    let start = process.hrtime.bigint()
    for (let i = 0; i < iterations; i++) {
        nqql.parse(q)
    }
    let end = process.hrtime.bigint()
    let elapsed = Number(end - start)
    return (iterations / elapsed) * 1e9
}

function bench_fast_json_parse(name, q, iterations) {
    for (let i = 0; i < 100; i++) {
        nqql.parseFastJson(q)
    }
    let start = process.hrtime.bigint()
    for (let i = 0; i < iterations; i++) {
        nqql.parseFastJson(q)
    }
    let end = process.hrtime.bigint()
    let elapsed = Number(end - start)
    return (iterations / elapsed) * 1e9
}

function bench_e2e(name, q, iterations) {
    // Explain builds the full execution payload offline (E2E pipeline)
    for (let i = 0; i < 100; i++) {
        nqql.explain(q)
    }
    let start = process.hrtime.bigint()
    for (let i = 0; i < iterations; i++) {
        nqql.explain(q)
    }
    let end = process.hrtime.bigint()
    let elapsed = Number(end - start)
    return (iterations / elapsed) * 1e9
}

const iterations = 10_000;
console.log(`Node.js nqql  |  ${iterations} iterations each\n`);
console.log(`${'Query'.padEnd(20)} | ${'NAPI parse()'.padStart(15)} | ${'parseFastJson()'.padStart(15)} | ${'E2E explain()'.padStart(15)}`);
console.log('-'.repeat(74));

for (const [name, q] of QUERIES) {
  const napi_parse = bench_napi_parse(name, q, iterations);
  const fast_parse = bench_fast_json_parse(name, q, iterations);
  const e2e = bench_e2e(name, q, iterations);
  console.log(`${name.padEnd(20)} | ${napi_parse.toFixed(0).padStart(15)} | ${fast_parse.toFixed(0).padStart(15)} | ${e2e.toFixed(0).padStart(15)}`);
}
