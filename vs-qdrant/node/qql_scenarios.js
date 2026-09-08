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
  async ingestBerlin(name, docs, dense, sparse) {
    const t0 = process.hrtime.bigint();
    const rows = docs.map((doc, k) => ({
      ...doc,
      vector: {
        dense: Array.from(dense.subarray(k * 384, (k + 1) * 384)),
        bm25: { indices: Array.from(sparse[k].indices), values: Array.from(sparse[k].values) },
      },
    }));
    await this.client.upsertMany(name, rows, { batchSize: BATCH_BERLIN });
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  async ingestLegal(name, docs, dense, sparse, colbertFlat, colbertLens) {
    const offsets = [0];
    for (const n of colbertLens) offsets.push(offsets[offsets.length - 1] + n * 128);
    const t0 = process.hrtime.bigint();
    const rows = docs.map((doc, k) => ({
      ...doc,
      vector: {
        dense: Array.from(dense.subarray(k * 384, (k + 1) * 384)),
        bm25: { indices: Array.from(sparse[k].indices), values: Array.from(sparse[k].values) },
        colbert: { data: Array.from(colbertFlat.subarray(offsets[k], offsets[k + 1])), dim: 128 },
      },
    }));
    await this.client.upsertMany(name, rows, { batchSize: BATCH_LEGAL });
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  // ------------------------------------------------------------- reads ----
  async queryDense(name, qvec) {
    return (await this.client.execute(`QUERY :dv FROM ${name} USING dense LIMIT 10`, { params: { dv: qvec } })).hits();
  }

  async queryDenseFiltered(name, qvec) {
    return (await this.client.execute(
      `QUERY :dv FROM ${name} USING dense WHERE price < 150.0 AND guests >= 2 LIMIT 10`,
      { params: { dv: qvec } },
    )).hits();
  }

  async querySparse(name, svec) {
    return (await this.client.execute(`QUERY :sv FROM ${name} USING bm25 LIMIT 10`, { params: { sv: svec } })).hits();
  }

  async queryHybrid(name, qvec, svec) {
    return (await this.client.execute(`
      WITH d AS (QUERY :dv FROM ${name} USING dense PARAMS (hnsw_ef = 128) LIMIT 50),
           s AS (QUERY :sv FROM ${name} USING bm25 LIMIT 50)
      QUERY FUSION RRF FROM ${name} PREFETCH (d, s) LIMIT 10`,
      { params: { dv: qvec, sv: svec } },
    )).hits();
  }

  async queryColbert(name, mvec) {
    return (await this.client.execute(`QUERY :mv FROM ${name} USING colbert LIMIT 10`, { params: { mv: mvec } })).hits();
  }

  async scrollPages(name, pages, batch) {
    const ids = [];
    let offset = null;
    for (let p = 0; p < pages; p++) {
      const sql = offset !== null ? `SCROLL FROM ${name} AFTER ${offset} LIMIT ${batch}` : `SCROLL FROM ${name} LIMIT ${batch}`;
      const rep = await this.client.execute(sql);
      const pageIds = rep.ids();
      if (!pageIds || pageIds.length === 0) break;
      ids.push(...pageIds);
      offset = ids[ids.length - 1];
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
      hits = (await this.client.execute(stmt, { params: { dv: v } })).hits();
    }
    return hits;
  }
}

module.exports = { QqlScenarios };
