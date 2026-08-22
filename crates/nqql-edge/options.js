"use strict";

/**
 * Wrapper option normalization for nqql-edge.
 *
 * Kept in its own module (no native binding required) so the mapping rules —
 * especially the sparse/multi/image/reranker model slots — are unit-testable.
 */

/**
 * Normalize the second arg of localExecutor:
 *   localExecutor(dir)                          → {}
 *   localExecutor(dir, false)                   → { onDiskPayload: false }  (legacy)
 *   localExecutor(dir, { model, onDiskPayload }) → as-is
 */
function normalizeLocalOptions(options) {
  if (options === undefined || options === null) {
    return {};
  }
  if (typeof options === "boolean") {
    return { onDiskPayload: options };
  }
  if (typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError(
      "localExecutor options must be a boolean (legacy onDiskPayload) or an object",
    );
  }
  return {
    onDiskPayload: options.onDiskPayload,
    model: typeof options.model === "string" ? options.model : undefined,
    sparseModel: typeof options.sparseModel === "string" ? options.sparseModel : undefined,
    multiModel: typeof options.multiModel === "string" ? options.multiModel : undefined,
    imageModel: typeof options.imageModel === "string" ? options.imageModel : undefined,
    rerankerModel:
      typeof options.rerankerModel === "string" ? options.rerankerModel : undefined,
    cacheDir: typeof options.cacheDir === "string" ? options.cacheDir : undefined,
    showDownloadProgress:
      typeof options.showDownloadProgress === "boolean"
        ? options.showDownloadProgress
        : undefined,
  };
}

function normalizeStandaloneOptions(options) {
  if (options === undefined || options === null) {
    return undefined;
  }
  if (typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("options must be an object");
  }
  if (
    options.onError !== undefined &&
    options.onError !== "stop" &&
    options.onError !== "continue"
  ) {
    throw new TypeError("options.onError must be 'stop' or 'continue'");
  }
  return {
    dataDir: typeof options.dataDir === "string" ? options.dataDir : "./qdrant_data",
    onDiskPayload: options.onDiskPayload ?? true,
    model: typeof options.model === "string" ? options.model : undefined,
    sparseModel: typeof options.sparseModel === "string" ? options.sparseModel : undefined,
    multiModel: typeof options.multiModel === "string" ? options.multiModel : undefined,
    imageModel: typeof options.imageModel === "string" ? options.imageModel : undefined,
    rerankerModel:
      typeof options.rerankerModel === "string" ? options.rerankerModel : undefined,
    cacheDir: typeof options.cacheDir === "string" ? options.cacheDir : undefined,
    showDownloadProgress:
      typeof options.showDownloadProgress === "boolean"
        ? options.showDownloadProgress
        : undefined,
    embedUrl: typeof options.embedUrl === "string" ? options.embedUrl : undefined,
    embedKey: typeof options.embedKey === "string" ? options.embedKey : undefined,
    embedModel: typeof options.embedModel === "string" ? options.embedModel : undefined,
    embedDim: Number.isSafeInteger(options.embedDim) ? options.embedDim : undefined,
    onError: options.onError,
  };
}

module.exports = { normalizeLocalOptions, normalizeStandaloneOptions };
