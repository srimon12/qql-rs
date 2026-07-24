const nqql = require('../crates/nqql/index.js');
let qqlWasm = null;
try {
  qqlWasm = require('../crates/qql-wasm/pkg-node/qql_wasm.js');
} catch (e) {
  console.warn("WASM package not found at crates/qql-wasm/pkg-node, running without WASM");
}

const decoder = new TextDecoder();

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

function run_bench(fn, iterations) {
  for (let i = 0; i < 100; i++) fn();
  const start = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) fn();
  const end = process.hrtime.bigint();
  const elapsed = Number(end - start);
  return (iterations / elapsed) * 1e9;
}

const iterations = 50_000;
console.log(`\n=== NODE.JS & WASM FAIR CONSUMER BENCHMARK SUITE (${iterations.toLocaleString()} iterations each) ===\n`);

const headers = [
  'Query'.padEnd(17),
  'NAPI parse()'.padStart(12),
  'NAPI parseJson()'.padStart(16),
  'WASM compileValue'.padStart(17),
  'WASM compile+JSON'.padStart(17),
  'WASM bytes+Decode'.padStart(17),
];

console.log(headers.join(' | '));
console.log('-'.repeat(headers.join(' | ').length));

for (const [name, q] of QUERIES) {
  const napi_parse = run_bench(() => nqql.parse(q), iterations);
  const napi_parse_json = run_bench(() => nqql.parseJson(q), iterations);

  // WASM Fair Consumer Paths:
  // 1. Direct JS Object via serde_wasm_bindgen
  const wasm_val = run_bench(() => qqlWasm.compileValue(q), iterations);
  // 2. String + V8 JSON.parse
  const wasm_str_json = run_bench(() => JSON.parse(qqlWasm.compile(q)), iterations);
  // 3. Safe Owned Uint8Array + TextDecoder + V8 JSON.parse
  const wasm_bytes_json = run_bench(() => JSON.parse(decoder.decode(qqlWasm.compileBytes(q))), iterations);

  const row = [
    name.padEnd(17),
    napi_parse.toFixed(0).padStart(12),
    napi_parse_json.toFixed(0).padStart(16),
    wasm_val.toFixed(0).padStart(17),
    wasm_str_json.toFixed(0).padStart(17),
    wasm_bytes_json.toFixed(0).padStart(17),
  ];

  console.log(row.join(' | '));
}
