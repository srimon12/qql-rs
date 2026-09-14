// nqql + qql-wasm throughput across the shared bench/queries.json corpus.
//
// Median-of-reps timing (default 50k iterations x 3 reps). Every query benches
// NAPI parse() / parseJson() / WASM parse(); the Bound query additionally
// benches the 0.3.2 prepared-statement path (bind, compileQuery, isValid).
//
// Usage:
//   node bench/bench_node.js [--iterations N] [--reps N] [--filter SUBSTR] [--json]
const fs = require('fs');
const path = require('path');

const nqql = require('../crates/nqql/index.js');
let qqlWasm = null;
try {
  qqlWasm = require('../crates/qql-wasm/pkg-node/qql_wasm.js');
} catch (e) {
  console.warn('WASM package not found at crates/qql-wasm/pkg-node, running without WASM');
}

const CORPUS = JSON.parse(fs.readFileSync(path.join(__dirname, 'queries.json'), 'utf8')).queries;

function parseArgs(argv) {
  const args = { iterations: 50_000, reps: 3, filter: null, json: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--help' || a === '-h') {
      console.log(
        'usage: node bench/bench_node.js [--iterations N] [--reps N] [--filter SUBSTR] [--json]\n\n' +
          'Benchmarks every query in bench/queries.json (median of N reps).\n' +
          `defaults: --iterations ${args.iterations} --reps ${args.reps}`,
      );
      process.exit(0);
    } else if (a === '--json') {
      args.json = true;
    } else if (a === '--iterations' || a === '--reps' || a === '--filter') {
      const v = argv[++i];
      if (v === undefined) {
        console.error(`${a} requires a value`);
        process.exit(2);
      }
      if (a === '--filter') args.filter = v;
      else {
        const n = Number(v);
        if (!Number.isInteger(n) || n < 1) {
          console.error(`invalid ${a} '${v}'`);
          process.exit(2);
        }
        args[a.slice(2)] = n;
      }
    } else {
      console.error(`unknown argument '${a}' (try --help)`);
      process.exit(2);
    }
  }
  return args;
}

let sink;

function timeOpsPerSec(fn, warmup, iterations) {
  for (let i = 0; i < warmup; i++) sink = fn();
  const start = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) sink = fn();
  const end = process.hrtime.bigint();
  return (iterations / Number(end - start)) * 1e9;
}

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

function bench(fn, warmup, iterations, reps) {
  const samples = [];
  for (let r = 0; r < reps; r++) samples.push(timeOpsPerSec(fn, warmup, iterations));
  return median(samples);
}

const args = parseArgs(process.argv.slice(2));
let queries = CORPUS;
if (args.filter) {
  const needle = args.filter.toLowerCase();
  queries = queries.filter((q) => q.name.toLowerCase().includes(needle));
  if (queries.length === 0) {
    console.error(`no queries match filter '${args.filter}' (see bench/queries.json)`);
    process.exit(2);
  }
}

const rows = [];
for (const q of queries) {
  const row = { query: q.name };
  row.parse = bench(() => nqql.parse(q.qql), 1000, args.iterations, args.reps);
  row.parseJson = bench(() => nqql.parseJson(q.qql), 1000, args.iterations, args.reps);
  if (qqlWasm) row.wasm = bench(() => qqlWasm.parse(q.qql), 1000, args.iterations, args.reps);
  rows.push(row);
}

// 0.3.2 prepared-statement path on the Bound query (if not filtered out).
const boundQ = queries.find((q) => q.name === 'Bound');
let boundRow = null;
if (boundQ) {
  boundRow = {
    bind: bench(() => nqql.bind(boundQ.qql, boundQ.params), 1000, args.iterations, args.reps),
    compileQuery: bench(
      () => nqql.compileQuery(boundQ.qql, boundQ.params),
      1000,
      args.iterations,
      args.reps,
    ),
    isValid: bench(() => nqql.isValid(boundQ.qql), 1000, args.iterations, args.reps),
  };
  if (qqlWasm) {
    boundRow.wasmBind = bench(
      () => qqlWasm.bind(boundQ.qql, boundQ.params),
      1000,
      args.iterations,
      args.reps,
    );
  }
}

if (args.json) {
  console.log(
    JSON.stringify({
      bin: 'bench_node',
      impl: `nqql ${require('../crates/nqql/package.json').version}`,
      iterations: args.iterations,
      reps: args.reps,
      results: rows,
      bound: boundRow,
    }),
  );
  process.exit(0);
}

console.log(
  `\n=== NODE.JS & WASM BENCHMARK (${args.iterations.toLocaleString()} iterations x ${args.reps} reps, median) ===\n`,
);
const headers = [
  'Query'.padEnd(18),
  'NAPI parse()'.padStart(13),
  'NAPI parseJson()'.padStart(17),
  ...(qqlWasm ? ['WASM parse()'.padStart(20)] : []),
];
console.log(headers.join(' | '));
console.log('-'.repeat(headers.join(' | ').length));
for (const r of rows) {
  console.log(
    [
      r.query.padEnd(18),
      r.parse.toFixed(0).padStart(13),
      r.parseJson.toFixed(0).padStart(17),
      ...(qqlWasm && r.wasm !== undefined ? [r.wasm.toFixed(0).padStart(20)] : []),
    ].join(' | '),
  );
}
if (boundRow) {
  console.log('\nBound query prepared-statement path (ops/s, median):');
  for (const [k, v] of Object.entries(boundRow)) console.log(`  ${k.padEnd(13)} ${v.toFixed(0)}`);
}
