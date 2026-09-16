"use strict";

/**
 * Wrapper option normalization for nqql.
 *
 * Kept in its own module (no native binding required) so the mapping rules —
 * especially the multi/image/rerank HTTP embedder fields — are unit-testable.
 */

/**
 * Normalize a Client / execute options object into the shape the native
 * binding expects. camelCase wins, snake_case is accepted as an alias.
 *
 * The embedder sub-object forwards the full HTTP embedder surface the native
 * layer supports: dense (`endpoint`/`apiKey`/`model`/`dimension`), multi
 * (ColBERT), image (CLIP), rerank (cross-encoder) endpoints, and the
 * client-side BM25 document params (`bm25K1`/`bm25B`/`bm25AvgLen`).
 */
function normalizeClientOptions(options) {
  if (!options) {
    return undefined;
  }
  if (typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("client options must be an object");
  }
  return {
    ...options,
    apiKey: options.apiKey ?? options.api_key,
    useGrpc: options.useGrpc ?? options.use_grpc,
    routeAffinity: options.routeAffinity ?? options.route_affinity,
    embedder: options.embedder
      ? {
          endpoint: options.embedder.endpoint,
          apiKey: options.embedder.apiKey ?? options.embedder.api_key,
          model: options.embedder.model,
          dimension: options.embedder.dimension,
          multiEndpoint:
            options.embedder.multiEndpoint ?? options.embedder.multi_endpoint,
          multiApiKey: options.embedder.multiApiKey ?? options.embedder.multi_api_key,
          multiModel: options.embedder.multiModel ?? options.embedder.multi_model,
          multiDimension:
            options.embedder.multiDimension ?? options.embedder.multi_dimension,
          imageEndpoint:
            options.embedder.imageEndpoint ?? options.embedder.image_endpoint,
          imageApiKey: options.embedder.imageApiKey ?? options.embedder.image_api_key,
          imageModel: options.embedder.imageModel ?? options.embedder.image_model,
          imageDimension:
            options.embedder.imageDimension ?? options.embedder.image_dimension,
          rerankEndpoint:
            options.embedder.rerankEndpoint ?? options.embedder.rerank_endpoint,
          rerankApiKey: options.embedder.rerankApiKey ?? options.embedder.rerank_api_key,
          rerankModel: options.embedder.rerankModel ?? options.embedder.rerank_model,
          // Client-side BM25 document params for the local sparse path
          // (write-path only; server-side inference is untouched).
          bm25K1: options.embedder.bm25K1 ?? options.embedder.bm25_k1,
          bm25B: options.embedder.bm25B ?? options.embedder.bm25_b,
          bm25AvgLen: options.embedder.bm25AvgLen ?? options.embedder.bm25_avg_len,
          bm25Language: options.embedder.bm25Language ?? options.embedder.bm25_language,
          bm25Tokenizer: options.embedder.bm25Tokenizer ?? options.embedder.bm25_tokenizer,
          bm25Lowercase: options.embedder.bm25Lowercase ?? options.embedder.bm25_lowercase,
          bm25AsciiFolding:
            options.embedder.bm25AsciiFolding ?? options.embedder.bm25_ascii_folding,
          bm25Stopwords: options.embedder.bm25Stopwords ?? options.embedder.bm25_stopwords,
          bm25Stemmer: options.embedder.bm25Stemmer ?? options.embedder.bm25_stemmer,
          bm25MinTokenLen:
            options.embedder.bm25MinTokenLen ?? options.embedder.bm25_min_token_len,
          bm25MaxTokenLen:
            options.embedder.bm25MaxTokenLen ?? options.embedder.bm25_max_token_len,
        }
      : undefined,
  };
}

module.exports = { normalizeClientOptions };
