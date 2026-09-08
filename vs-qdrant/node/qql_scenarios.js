/**
 * QQL SDK (nqql) scenario implementations.
 *
 * One method per benchmark scenario — the exact application code a QQL user
 * writes, symmetric with official_scenarios.js.
 *
 * Notes:
 *   * QUERY parameters (:dv / :sv / :mv) bind per call — parse once, rerun
 *     with different vectors (prepared statements).
 *   * Bulk ingest is a thin bind loop over one `:rows` template (typed
 *     views splice with one memcpy; the async upsertMany serde boundary
 *     would force plain-array copies — see method comment).
 */
const nqql = require("./nqql/index.js");

const BATCH_BERLIN = 100;
const BATCH_LEGAL = 32;


class QqlScenarios {
  constructor(url) {
    this.client = new nqql.Client({ url });
  }

  async close() {
    await this.client.close();
  }

  // ------------------------------------------------------------ setup ----
  async dropCollection(name) {
    try { await this.client.execute(`DROP COLLECTION ${name}`); } catch (_) { /* absent */ }
  }

  async createBerlin(name) {
    await this.client.execute(`
      CREATE COLLECTION ${name} (
        dense VECTOR(384, COSINE),
        bm25 SPARSE
      )`);
    await this.client.execute(`CREATE INDEX ON COLLECTION ${name} FOR district TYPE keyword`);
    await this.client.execute(`CREATE INDEX ON COLLECTION ${name} FOR price TYPE float`);
  }

  async createLegal(name) {
    await this.client.execute(`
      CREATE COLLECTION ${name} (
        dense VECTOR(384, COSINE),
        bm25 SPARSE,
        colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH HNSW (m = 0)
      )`);
    await this.client.execute(`CREATE INDEX ON COLLECTION ${name} FOR court TYPE keyword`);
    await this.client.execute(`CREATE INDEX ON COLLECTION ${name} FOR year TYPE integer`);
  }

  // ----------------------------------------------------------- ingest ----
  // Typed arrays only convert on the sync Stmt.bind surface (the async
  // upsertMany boundary is serde: Float32Array would arrive index-keyed).
  // So ingest stays a bind loop — but a thin one: constant template,
  // one dict-splice per batch, no text rows, no helpers.
  async ingestBerlin(name, docs, dense, sparse) {
    const t0 = process.hrtime.bigint();
    const rows = docs.map((doc, k) => ({ ...doc, vector: {
      dense: Array.from(dense.subarray(k * 384, (k + 1) * 384)),
      bm25: { indices: Array.from(sparse[k].indices), values: Array.from(sparse[k].values) },
    }}));
    await this.client.upsertMany(name, rows, { batchSize: BATCH_BERLIN });
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  async ingestLegal(name, docs, dense, sparse, colbertFlat, colbertLens) {
    const offsets = [0];
    for (const n of colbertLens) offsets.push(offsets[offsets.length - 1] + n * 128);
    const t0 = process.hrtime.bigint();
    const rows = docs.map((doc, k) => ({ ...doc, vector: {
      dense: Array.from(dense.subarray(k * 384, (k + 1) * 384)),
      bm25: { indices: Array.from(sparse[k].indices), values: Array.from(sparse[k].values) },
      colbert: { data: Array.from(colbertFlat.subarray(offsets[k], offsets[k + 1])), dim: 128 },
    }}));
    await this.client.upsertMany(name, rows, { batchSize: BATCH_LEGAL });
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  // ------------------------------------------------------------- reads ----
  _hits(report) {
    return report.results[0].data;
  }

  async queryDense(name, qvec) {
    const stmt = nqql.parse(`QUERY :dv FROM ${name} USING dense LIMIT 10`)[0];
    return this._hits(await this.client.execute(stmt, { params: { dv: qvec } }));
  }

  async queryDenseFiltered(name, qvec) {
    const stmt = nqql.parse(
      `QUERY :dv FROM ${name} USING dense WHERE price < 150.0 AND guests >= 2 LIMIT 10`)[0];
    return this._hits(await this.client.execute(stmt, { params: { dv: qvec } }));
  }

  async querySparse(name, svec) {
    const stmt = nqql.parse(`QUERY :sv FROM ${name} USING bm25 LIMIT 10`)[0];
    return this._hits(await this.client.execute(stmt, { params: { sv: svec } }));
  }

  async queryHybrid(name, qvec, svec) {
    // hnsw_ef=128 on the dense CTE: fused rankings must be deterministic
    // across the two independently built collections.
    const stmt = nqql.parse(`
      WITH d AS (QUERY :dv FROM ${name} USING dense PARAMS (hnsw_ef = 128) LIMIT 50),
           s AS (QUERY :sv FROM ${name} USING bm25 LIMIT 50)
      QUERY FUSION RRF FROM ${name} PREFETCH (d, s) LIMIT 10`)[0];
    return this._hits(await this.client.execute(stmt, { params: { dv: qvec, sv: svec } }));
  }

  async queryColbert(name, mvec) {
    const stmt = nqql.parse(`QUERY :mv FROM ${name} USING colbert LIMIT 10`)[0];
    return this._hits(await this.client.execute(stmt, { params: { mv: mvec } }));
  }

  async scrollPages(name, pages, batch) {
    const stmt = nqql.parse(`SCROLL FROM ${name} AFTER :off LIMIT ${batch}`)[0];
    const ids = [];
    let off = 0;
    for (let p = 0; p < pages; p++) {
      const hits = this._hits(await this.client.execute(stmt, { params: { off } }));
      if (!hits.length) break;
      for (const h of hits) ids.push(h.id);
      // QQL `AFTER` is exclusive — resume at the boundary point.
      off = Number(ids[ids.length - 1]);
    }
    return ids;
  }

  async countBerlin(name) {
    const report = await this.client.execute(`COUNT FROM ${name} WHERE price < 150.0`);
    return report.count();
  }

  async countLegal(name) {
    const report = await this.client.execute(`COUNT FROM ${name} WHERE year >= 2010`);
    return report.count();
  }

  async facetDistrict(name) {
    const report = await this.client.execute(`FACET district FROM ${name} LIMIT 20 EXACT true`);
    const map = {};
    for (const h of report.facet()) map[h.value] = h.count;
    return map;
  }

  // ----------------------------------------------------------- writes ----
  async updatePayload(name) {
    await this.client.execute(
      `UPDATE ${name} SET PAYLOAD = {rating: 4.5} WHERE district = 'Mitte' WAIT true`);
  }

  async deleteByFilter(name) {
    await this.client.execute(`DELETE FROM ${name} WHERE price > 250.0 WAIT true`);
  }

  async preparedRerun(name, qvecs) {
    // Parse once, rerun with different vectors — the prepared path.
    const stmt = nqql.parse(`QUERY :dv FROM ${name} USING dense LIMIT 10`)[0];
    let hits;
    for (const v of qvecs) {
      hits = this._hits(await this.client.execute(stmt, { params: { dv: v } }));
    }
    return hits;
  }
}

module.exports = { QqlScenarios };
