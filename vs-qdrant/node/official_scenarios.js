/**
 * Official Qdrant SDK (@qdrant/js-client-rest) scenario implementations.
 *
 * One method per benchmark scenario — the exact application code a Qdrant
 * JS user writes. Symmetric with qql_scenarios.js; the harness measures both.
 */
const { QdrantClient } = require("@qdrant/js-client-rest");

const BATCH_BERLIN = 100;
const BATCH_LEGAL = 32;

class OfficialScenarios {
  constructor(url) {
    this.client = new QdrantClient({ url, checkCompatibility: false, timeout: 120_000 });
  }

  async close() {
    // js-client has no explicit close
  }

  // ------------------------------------------------------------ setup ----
  async dropCollection(name) {
    try { await this.client.deleteCollection(name); } catch (_) { /* absent */ }
  }

  async createBerlin(name) {
    await this.client.createCollection(name, {
      vectors: { dense: { size: 384, distance: "Cosine" } },
      sparse_vectors: { bm25: { modifier: "idf" } },
    });
    await this.client.createPayloadIndex(name, { field_name: "district", field_schema: "keyword" });
    await this.client.createPayloadIndex(name, { field_name: "price", field_schema: "float" });
  }

  async createLegal(name) {
    await this.client.createCollection(name, {
      vectors: {
        dense: { size: 384, distance: "Cosine" },
        colbert: {
          size: 128, distance: "Cosine",
          multivector_config: { comparator: "max_sim" },
          hnsw_config: { m: 0 },
        },
      },
      sparse_vectors: { bm25: { modifier: "idf" } },
    });
    await this.client.createPayloadIndex(name, { field_name: "court", field_schema: "keyword" });
    await this.client.createPayloadIndex(name, { field_name: "year", field_schema: "integer" });
  }

  // ----------------------------------------------------------- ingest ----
  _points(docs, dense, sparse, colbert) {
    return docs.map((doc, i) => {
      const vector = {
        dense: Array.from(dense.subarray(i * 384, (i + 1) * 384)),
        bm25: { indices: sparse[i].indices, values: sparse[i].values },
      };
      if (colbert) vector.colbert = colbert[i];
      const { id, ...payload } = doc;
      return { id, vector, payload };
    });
  }

  async ingestBerlin(name, docs, dense, sparse) {
    const points = this._points(docs, dense, sparse, null);
    const t0 = process.hrtime.bigint();
    for (let i = 0; i < points.length; i += BATCH_BERLIN) {
      await this.client.upsert(name, { points: points.slice(i, i + BATCH_BERLIN), wait: false });
    }
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  async ingestLegal(name, docs, dense, sparse, colbert) {
    const points = this._points(docs, dense, sparse, colbert);
    const t0 = process.hrtime.bigint();
    for (let i = 0; i < points.length; i += BATCH_LEGAL) {
      await this.client.upsert(name, { points: points.slice(i, i + BATCH_LEGAL), wait: false });
    }
    return Number(process.hrtime.bigint() - t0) / 1e9;
  }

  // ------------------------------------------------------------- reads ----
  async queryDense(name, qvec) {
    return (await this.client.query(name, {
      query: qvec, using: "dense", limit: 10, with_payload: true,
    })).points;
  }

  async queryDenseFiltered(name, qvec) {
    return (await this.client.query(name, {
      query: qvec, using: "dense", limit: 10, with_payload: true,
      filter: {
        must: [
          { key: "price", range: { lt: 150.0 } },
          { key: "guests", range: { gte: 2 } },
        ],
      },
    })).points;
  }

  async querySparse(name, svec) {
    return (await this.client.query(name, {
      query: { indices: svec.indices, values: svec.values },
      using: "bm25", limit: 10, with_payload: true,
    })).points;
  }

  async queryHybrid(name, qvec, svec) {
    // hnsw_ef=128 on the dense leg: fused rankings must be deterministic
    // across the two independently built collections.
    return (await this.client.query(name, {
      prefetch: [
        { query: qvec, using: "dense", limit: 50, params: { hnsw_ef: 128 } },
        { query: { indices: svec.indices, values: svec.values }, using: "bm25", limit: 50 },
      ],
      query: { fusion: "rrf" },
      limit: 10, with_payload: true,
    })).points;
  }

  async queryColbert(name, mvec) {
    return (await this.client.query(name, {
      query: mvec, using: "colbert", limit: 10, with_payload: true,
    })).points;
  }

  async scrollPages(name, pages, batch) {
    const ids = [];
    let offset;
    for (let p = 0; p < pages; p++) {
      const { points, next_page_offset: next } = await this.client.scroll(name, {
        limit: batch, offset, with_payload: true,
      });
      for (const pt of points) ids.push(pt.id);
      offset = next;
      if (offset === undefined || offset === null) break;
    }
    return ids;
  }

  async countBerlin(name) {
    return (await this.client.count(name, {
      filter: { must: [{ key: "price", range: { lt: 150.0 } }] }, exact: true,
    })).count;
  }

  async countLegal(name) {
    return (await this.client.count(name, {
      filter: { must: [{ key: "year", range: { gte: 2010 } }] }, exact: true,
    })).count;
  }

  async facetDistrict(name) {
    const res = await this.client.facet(name, { key: "district", limit: 20, exact: true });
    const map = {};
    for (const h of res.hits) map[h.value] = h.count;
    return map;
  }

  // ----------------------------------------------------------- writes ----
  async updatePayload(name) {
    await this.client.setPayload(name, {
      payload: { rating: 4.5 },
      filter: { must: [{ key: "district", match: { value: "Mitte" } }] },
      wait: true,
    });
  }

  async deleteByFilter(name) {
    await this.client.delete(name, {
      filter: { must: [{ key: "price", range: { gt: 250.0 } }] },
      wait: true,
    });
  }

  async preparedRerun(name, qvecs) {
    // No prepared statements in the official SDK: repeat the call.
    let hits;
    for (const v of qvecs) {
      hits = (await this.client.query(name, {
        query: v, using: "dense", limit: 10, with_payload: true,
      })).points;
    }
    return hits;
  }
}

module.exports = { OfficialScenarios };
