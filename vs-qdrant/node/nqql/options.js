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
 * (ColBERT), image (CLIP), and rerank (cross-encoder) endpoints.
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
        }
      : undefined,
  };
}

module.exports = { normalizeClientOptions };
