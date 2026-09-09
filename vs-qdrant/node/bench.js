/**
 * Head-to-head harness: official @qdrant/js-client-rest vs nqql (QQL).
 * Same design as python/bench.py — identical scenarios, parity-checked.
 *
 * Usage: node bench.js [--reps 5] [--iters 25]
 * Writes: ../results/node.json
 */
const fs = require("fs");
const path = require("path");
const { performance } = require("perf_hooks");
const { OfficialScenarios } = require("./official_scenarios.js");
const { QqlScenarios } = require("./qql_scenarios.js");

const ROOT = path.resolve(__dirname, "..");
const DATA = path.join(ROOT, "data");
const RESULTS = path.join(ROOT, "results");
const URL = "http://localhost:6333";
const SCROLL_PAGES = 3;
const SCROLL_BATCH = 256;

// ------------------------------------------------------------------ data ----
function loadData() {
  const lines = (f) => fs.readFileSync(path.join(DATA, f), "utf8").split("\n").filter(Boolean);
  const docs = {
    berlin: lines("berlin.jsonl").map((l) => JSON.parse(l)),
    legal: lines("legal.jsonl").map((l) => JSON.parse(l)),
  };
  const f32 = (f, n, d) => {
    const buf = fs.readFileSync(path.join(DATA, f));
    const a = new Float32Array(n * d);
    for (let i = 0; i < n * d; i++) a[i] = buf.readFloatLE(i * 4);
    return a;
  };
  const dense = {
    berlin: f32("berlin_dense.f32", docs.berlin.length, 384),
    legal: f32("legal_dense.f32", docs.legal.length, 384),
  };
  const sparse = {
    berlin: JSON.parse(fs.readFileSync(path.join(DATA, "berlin_bm25.json"), "utf8")),
    legal: JSON.parse(fs.readFileSync(path.join(DATA, "legal_bm25.json"), "utf8")),
  };
  const lens = JSON.parse(fs.readFileSync(path.join(DATA, "legal_colbert_lens.json"), "utf8"));
  const colbertFlatBuf = fs.readFileSync(path.join(DATA, "legal_colbert.f32"));
  const colbertFlat = new Float32Array(lens.reduce((a, b) => a + b, 0) * 128);
  for (let i = 0; i < colbertFlat.length; i++) colbertFlat[i] = colbertFlatBuf.readFloatLE(i * 4);
  // official client ingests nested per-token rows (its PointStruct path is
  // plain JSON arrays); qql side uses the flat Float32Array above.
  const colbertRows = [];
  let o = 0;
  for (const n of lens) {
    const rows = new Array(n);
    for (let t = 0; t < n; t++) {
      const row = new Array(128);
      for (let d = 0; d < 128; d++) row[d] = colbertFlatBuf.readFloatLE((o + t * 128 + d) * 4);
      rows[t] = row;
    }
    colbertRows.push(rows);
    o += n * 128;
  }
  // qql side binds typed arrays (one memcpy); keep a plain-array copy of the
  // sparse terms for the official client's JSON PointStruct path.
  for (const k of ["berlin", "legal"]) {
    sparse[k] = sparse[k].map((sv) => ({
      indicesPlain: sv.indices, valuesPlain: sv.values,
    }));
  }
  const queries = JSON.parse(fs.readFileSync(path.join(DATA, "queries.json"), "utf8"));
  return { docs, dense, sparse, colbertFlat, colbertLens: lens, colbertRows, queries };
}

// --------------------------------------------------------------- timing ----
async function timedRead(fn, reps, iters) {
  const samples = [];
  for (let r = 0; r < reps; r++) {
    if (r === 0) await fn(); // warmup rep untimed
    const t0 = performance.now();
    for (let i = 0; i < iters; i++) await fn();
    samples.push((performance.now() - t0) / iters);
  }
  samples.sort((a, b) => a - b);
  const med = samples[Math.floor(samples.length / 2)];
  return {
    ops_per_sec: Math.round(1000 / med),
    p50_ms: +med.toFixed(3),
    p95_ms: +samples[samples.length - 1].toFixed(3),
    reps, iters,
  };
}

async function waitUntilReady(collection, expected) {
  // Untimed barrier: count visible AND collection green (optimizer idle).
  // Points-count alone races background merges/HNSW indexing, which makes
  // early scenarios slower than later ones on identical data (variance).
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    const res = await fetch(`${URL}/collections/${collection}`);
    const info = (await res.json()).result;
    if (info.points_count === expected && info.status === "green") return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`${collection} never reached ${expected} points + green`);
}

// --------------------------------------------------------------- parity ----
function normHits(hits) {
  return hits.map((h) => (typeof h === "object" && !(h instanceof Array) && "id" in h && typeof h.score === "number"
    ? { id: h.id, score: h.score, payload: h.payload }
    : h));
}

function hitKey(h) {
  const n = Number(h.id);
  return Number.isFinite(n) ? n : h.id;
}

function compareHits(official, qql) {
  const o = normHits(official), q = normHits(qql);
  const oIds = o.map(hitKey), qIds = q.map(hitKey);
  const oSet = new Set(oIds), qSet = new Set(qIds);
  let inter = 0;
  for (const id of oSet) if (qSet.has(id)) inter++;
  const overlap = inter / Math.max(1, oSet.size + qSet.size - inter);
  const oScores = new Map(o.map((h) => [hitKey(h), +Number(h.score).toFixed(5)]));
  let maxDiff = 0;
  for (const [k, v] of oScores) {
    const qh = q.find((h) => hitKey(h) === k);
    if (qh) maxDiff = Math.max(maxDiff, Math.abs(v - Number(qh.score)));
  }
  return { top1_match: !!oIds.length && !!qIds.length && oIds[0] === qIds[0],
           jaccard_overlap: +overlap.toFixed(3), max_score_diff: +maxDiff.toFixed(6) };
}

function compareExact(official, qql) {
  const ok = JSON.stringify(official) === JSON.stringify(qql);
  return { match: ok, detail: ok ? "" : `${JSON.stringify(official).slice(0, 100)} != ${JSON.stringify(qql).slice(0, 100)}` };
}

async function restPoint(collection, pointId) {
  const res = await fetch(`${URL}/collections/${collection}/points`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ids: [pointId], with_payload: true, with_vector: true }),
  });
  return (await res.json()).result[0];
}

function arraysClose(a, b, tol = 1e-6) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (Math.abs(a[i] - b[i]) > tol) return false;
  return true;
}

// ------------------------------------------------------------------ main ----
async function main() {
  const args = { reps: 5, iters: 25 };
  process.argv.forEach((a, i, argv) => {
    if (a === "--reps") args.reps = +argv[i + 1];
    if (a === "--iters") args.iters = +argv[i + 1];
  });

  const { docs, dense, sparse, colbertFlat, colbertLens, colbertRows, queries } = loadData();
  // qql-side sparse terms as typed arrays (Int32Array/Float32Array = one memcpy)
  const sparseQql = {
    berlin: sparse.berlin.map((sv) => ({ indices: new Int32Array(sv.indicesPlain), values: new Float32Array(sv.valuesPlain) })),
    legal: sparse.legal.map((sv) => ({ indices: new Int32Array(sv.indicesPlain), values: new Float32Array(sv.valuesPlain) })),
  };
  // official-side sparse terms as plain arrays (JSON PointStruct path)
  const sparseOff = {
    berlin: sparse.berlin.map((sv) => ({ indices: sv.indicesPlain, values: sv.valuesPlain })),
    legal: sparse.legal.map((sv) => ({ indices: sv.indicesPlain, values: sv.valuesPlain })),
  };
  const official = new OfficialScenarios(URL);
  const qql = new QqlScenarios(URL);
  const R = { meta: await meta(), scenarios: {}, ingest: {}, parity: {}, cold_import: {} };

  // cold import (fresh process, median of 5)
  R.cold_import = {
    "qdrant/js-client-rest": coldRequireMs("@qdrant/js-client-rest"),
    nqql: coldRequireMs("./nqql/index.js"),
  };

  const COLLS = {
    berlin: { official: "vsq_berlin_official", qql: "vsq_berlin_qql" },
    legal: { official: "vsq_legal_official", qql: "vsq_legal_qql" },
  };

  for (const side of ["official", "qql"]) {
    const impl = side === "official" ? official : qql;
    await impl.dropCollection(COLLS.berlin[side]);
    await impl.dropCollection(COLLS.legal[side]);
  }
  await official.createBerlin(COLLS.berlin.official);
  await official.createLegal(COLLS.legal.official);
  await qql.createBerlin(COLLS.berlin.qql);
  await qql.createLegal(COLLS.legal.qql);

  // ingest (timed once per side)
  let t0 = performance.now();
  await official.ingestBerlin(COLLS.berlin.official, docs.berlin, dense.berlin, sparseOff.berlin);
  await official.ingestLegal(COLLS.legal.official, docs.legal, dense.legal, sparseOff.legal, colbertRows);
  R.ingest.official_sec = +((performance.now() - t0) / 1000).toFixed(3);
  t0 = performance.now();
  await qql.ingestBerlin(COLLS.berlin.qql, docs.berlin, dense.berlin, sparseQql.berlin);
  await qql.ingestLegal(COLLS.legal.qql, docs.legal, dense.legal,
    sparseQql.legal, colbertFlat, colbertLens);
  R.ingest.qql_sec = +((performance.now() - t0) / 1000).toFixed(3);
  R.ingest.points = { berlin: docs.berlin.length, legal: docs.legal.length,
                      batch: { berlin: 100, legal: 32 } };
  console.log(`ingest: official ${R.ingest.official_sec}s vs qql ${R.ingest.qql_sec}s`);

  for (const side of ["official", "qql"])
    for (const dom of ["berlin", "legal"])
      await waitUntilReady(COLLS[dom][side], docs[dom].length);

  // vector/payload parity of an ingested point (raw REST, both sides)
  const canon = (o) => JSON.stringify(o, Object.keys(o || {}).sort());
  for (const [dom, pid] of [["berlin", 1], ["legal", 1]]) {
    const o = await restPoint(COLLS[dom].official, pid);
    const q = await restPoint(COLLS[dom].qql, pid);
    let ok = canon(o.payload) === canon(q.payload)
      && new Set(Object.keys(o.vector)).size === new Set(Object.keys(q.vector)).size;
    if (ok) {
      for (const k of Object.keys(o.vector)) {
        const a = o.vector[k], b = q.vector[k];
        if (Array.isArray(a)) ok = ok && arraysClose(a, b);
        else ok = ok && a.indices.join(",") === b.indices.join(",")
          && arraysClose(a.values, b.values);
      }
    }
    R.parity[`ingested_point_${dom}`] = { match: !!ok };
    if (!ok) console.log(`PARITY FAIL ingested_point_${dom}`);
  }

  const S = R.scenarios;
  const B_OFF = COLLS.berlin.official, B_QQL = COLLS.berlin.qql;
  const L_OFF = COLLS.legal.official, L_QQL = COLLS.legal.qql;
  const bq = queries.berlin;

  async function readPair(label, oFn, qFn, parityFn = compareHits) {
    S[label] = {
      official: await timedRead(oFn, args.reps, args.iters),
      qql: await timedRead(qFn, args.reps, args.iters),
    };
    S[label].parity = await parityFn(await oFn(), await qFn());
    const ratio = S[label].qql.ops_per_sec / S[label].official.ops_per_sec;
    console.log(`${label.padEnd(24)} official ${String(S[label].official.ops_per_sec).padStart(9)} | `
      + `qql ${String(S[label].qql.ops_per_sec).padStart(9)} | x${ratio.toFixed(2)} `
      + `| parity ${JSON.stringify(S[label].parity).slice(0, 90)}`);
  }

  await readPair("query_dense",
    () => official.queryDense(B_OFF, bq[0].dense),
    () => qql.queryDense(B_QQL, bq[0].dense));
  await readPair("query_dense_filtered",
    () => official.queryDenseFiltered(B_OFF, bq[1].dense),
    () => qql.queryDenseFiltered(B_QQL, bq[1].dense));
  await readPair("query_sparse",
    () => official.querySparse(B_OFF, bq[0].sparse),
    () => qql.querySparse(B_QQL, bq[0].sparse));
  await readPair("query_hybrid",
    () => official.queryHybrid(B_OFF, bq[2].dense, bq[2].sparse),
    () => qql.queryHybrid(B_QQL, bq[2].dense, bq[2].sparse),
    async (o1, q) => ({
      qql_vs_official: compareHits(o1, q),
      official_vs_official: compareHits(o1,
        await official.queryHybrid(B_OFF, bq[2].dense, bq[2].sparse)),
    }));
  await readPair("scroll_pages",
    () => official.scrollPages(B_OFF, SCROLL_PAGES, SCROLL_BATCH),
    () => qql.scrollPages(B_QQL, SCROLL_PAGES, SCROLL_BATCH),
    (a, b) => compareHits(a.map((id) => ({ id, score: 0 })), b.map((id) => ({ id, score: 0 }))));
  await readPair("count_berlin",
    () => official.countBerlin(B_OFF), () => qql.countBerlin(B_QQL), compareExact);
  await readPair("facet_district",
    () => official.facetDistrict(B_OFF), () => qql.facetDistrict(B_QQL), compareExact);
  await readPair("query_colbert",
    () => official.queryColbert(L_OFF, queries.legal[0].colbert),
    () => qql.queryColbert(L_QQL, queries.legal[0].colbert));
  await readPair("count_legal",
    () => official.countLegal(L_OFF), () => qql.countLegal(L_QQL), compareExact);
  await readPair("prepared_rerun",
    () => official.preparedRerun(B_OFF, bq.map((q) => q.dense)),
    () => qql.preparedRerun(B_QQL, bq.map((q) => q.dense)));

  // writes (last, parity-checked)
  await official.updatePayload(B_OFF);
  await qql.updatePayload(B_QQL);
  R.parity.update_payload = compareExact(
    await official.facetDistrict(B_OFF), await qql.facetDistrict(B_QQL)).match;

  await official.deleteByFilter(B_OFF);
  await qql.deleteByFilter(B_QQL);
  const expected = docs.berlin.filter((d) => d.price <= 250.0).length;
  for (const side of ["official", "qql"])
    await waitUntilReady(COLLS.berlin[side], expected);
  R.parity.delete_by_filter = compareExact(
    await official.countBerlin(B_OFF), await qql.countBerlin(B_QQL)).match;

  fs.mkdirSync(RESULTS, { recursive: true });
  fs.writeFileSync(path.join(RESULTS, "node.json"), JSON.stringify(R, null, 2));
  console.log(`\nwritten ${path.join(RESULTS, "node.json")}`);

  await official.close();
  await qql.close();
}

function CollsName(COLLS, dom, side) {
  return COLLS[dom][side];
}

function coldRequireMs(spec, reps = 5) {
  const { spawnSync } = require("child_process");
  const samples = [];
  for (let i = 0; i < reps; i++) {
    const t0 = performance.now();
    spawnSync(process.execPath, ["-e", `require(${JSON.stringify(spec)})`], {
      cwd: __dirname, stdio: "ignore",
    });
    samples.push(performance.now() - t0);
  }
  samples.sort((a, b) => a - b);
  return +samples[Math.floor(samples.length / 2)].toFixed(1);
}

async function meta() {
  const qdrant = await (await fetch(URL)).json();
  const jsPkg = JSON.parse(fs.readFileSync(
    path.join(__dirname, "node_modules", "@qdrant", "js-client-rest", "package.json"), "utf8"));
  return {
    timestamp: new Date().toISOString(),
    node: process.version,
    nqql: require("./nqql/index.js").version,
    js_client_rest: jsPkg.version,
    qdrant_server: qdrant.version,
    url: URL,
  };
}

main().catch((e) => { console.error(e); process.exit(1); });
