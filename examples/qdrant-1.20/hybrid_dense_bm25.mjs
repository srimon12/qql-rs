/**
 * Hybrid search on Qdrant 1.20: dense leg + server-side BM25 leg, RRF fusion.
 *
 * DRAFT: proposed QQL syntax (docs/design/bm25-server-side-1.20.md). Each
 * PREFETCH leg runs exactly one scorer: the dense leg embeds, and the BM25 leg is
 * a Qdrant `text` query on the `title` full-text index. No client sparse
 * encoding happens, so nothing is scored twice.
 *
 * Offline: node examples/qdrant-1.20/hybrid_dense_bm25.mjs
 */
import nqql from '../../crates/nqql/index.js';

const { isValid, compile } = nqql;

const COLLECTION = 'movies_hybrid';

// Swap to Core Rust's final spelling here.
const STATEMENTS = [
  `CREATE COLLECTION ${COLLECTION} (dense VECTOR(384, COSINE))`,
  `CREATE INDEX ON COLLECTION ${COLLECTION} FOR title TYPE text WITH (scoring = 'bm25', tokenizer = 'word')`,
  `WITH
     dense AS (QUERY TEXT 'time travel' USING dense LIMIT 50),
     lexical AS (QUERY BM25 'time travel' USING title PARAMS (k = 1.2, b = 0.75) LIMIT 50)
   QUERY FUSION RRF FROM ${COLLECTION} PREFETCH (dense, lexical) LIMIT 10`,
];

for (const stmt of STATEMENTS) {
  const ok = isValid(stmt);
  console.log(`[${ok ? 'ok' : 'pending QQL grammar'}] ${stmt.replace(/\s+/g, ' ')}`);
  if (ok) {
    const route = compile(stmt);
    console.log(`      -> ${route.method} ${route.path}`);
  }
}
