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
const nqql = require("./index.js");

const fs = require("fs");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "nqql-warm-"));

const cacheRoot = process.env.HF_HOME
  ? process.env.HF_HOME
  : process.env.FASTEMBED_CACHE_DIR || path.join(".fastembed_cache");
console.log("warming fastembed cache:", path.resolve(cacheRoot));

try {
  nqql.localExecutor(tmp);
  console.log("model cache ready (default model initialized)");
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
