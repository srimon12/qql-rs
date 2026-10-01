"use strict";

/**
 * CI helper: warm the default fastembed cache with the default embedding
 * model so test runs never race HuggingFace rate limits.
 *
 * Deliberately passes NO cacheDir — mirrors the un-guarded `localExecutor`
 * call paths in test.js/test_comprehensive.js, so the model lands in the
 * hf-hub default cache (~/.cache/huggingface/hub) that CI caches.
 * Retry/backoff is owned by the caller (see ci.yml `nqql-check`).
 */

const os = require("os");
const path = require("path");
const fs = require("fs");
const nqql = require("./index.js");

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "nqql-warm-"));

async function main() {
  // The cache root is whatever fastembed resolves (HF_HOME > FASTEMBED_CACHE_DIR
  // > ./.fastembed_cache); CI pins HF_HOME, so do not log a guess here.
  const client = await nqql.localExecutor(tmp);
  // Close before deleting the data dir: the native executor holds the shard
  // and WAL files open until the executor is flushed.
  await client.close();
  console.log("model cache ready (default model initialized)");
}

main()
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => {
    fs.rmSync(tmp, { recursive: true, force: true });
  });
