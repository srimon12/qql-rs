const pkg = require('./package.json');

function isMusl() {
  if (process.platform !== 'linux') {
    return false;
  }
  const report = process.report?.getReport?.();
  return !report?.header?.glibcVersionRuntime;
}

function nativeTarget() {
  const { platform, arch } = process;
  if (platform === 'linux' && (arch === 'x64' || arch === 'arm64')) {
    if (isMusl()) {
      throw new Error(`nqql-edge ${pkg.version} does not provide a Linux musl binary`);
    }
    return arch === 'arm64' ? 'linux-arm64-gnu' : 'linux-x64-gnu';
  }
  if (platform === 'darwin' && arch === 'arm64') {
    return 'darwin-arm64';
  }
  if (platform === 'darwin' && arch === 'x64') {
    throw new Error(
      `nqql-edge ${pkg.version} does not provide a macOS Intel binary because ONNX Runtime no longer ships the required x86_64 artifact`,
    );
  }
  if (platform === 'win32' && arch === 'x64') {
    return 'win32-x64-msvc';
  }
  throw new Error(`nqql-edge does not provide a native binary for ${platform}-${arch}`);
}

const target = nativeTarget();
let nativeBinding;
try {
  nativeBinding = require(`./index.${target}.node`);
} catch (localError) {
  try {
    nativeBinding = require(`@veristamp/nqql-edge-${target}`);
  } catch (packageError) {
    try {
      // Local development fallback for a non-platform napi build.
      nativeBinding = require('./index.node');
    } catch (_) {
      packageError.cause = localError;
      throw packageError;
    }
  }
}

// Shared DX layer (error mapping, typed result classes, Stmt-aware bind) —
// byte-identical with nqql-edge; a CI check diffs the two copies.
const dx = require('./dx-common.js');
dx.installStmtToJSON(nativeBinding.Stmt);

const { buildError, callNative, validateOptions, ScoredPoint, ExecutionReport, scrollCursor, scrollStream } = dx;

const normalizeQuery = (query) => dx.normalizeQuery(nativeBinding.Stmt, query);




/**
 * Parse one statement or a semicolon-delimited script.
 * Always returns Stmt[].
 */
function parse(query) {
  return callNative(() => nativeBinding.parseAll(query));
}

/**
 * Fast raw JSON parse — bypasses V8 object allocation entirely.
 * Returns a JSON string of the AST array.  Ideal for HTTP/IPC forwarding.
 */
function parseJson(query) {
  return callNative(() => nativeBinding.parseAllJson(query));
}

function isValid(query) {
  return nativeBinding.isValid(query);
}

function injectFilter(query, field, op, value) {
  return callNative(() => nativeBinding.injectFilter(query, field, op, value));
}


function tokenize(query) {
  return callNative(() => nativeBinding.tokenize(query));
}

function explain(query) {
  return callNative(() => nativeBinding.explain(query));
}

function explainStmt(stmt) {
  return callNative(() => nativeBinding.explainStmt(stmt));
}

function compileQuery(query, params) {
  return callNative(() => nativeBinding.compileQuery(query, params));
}

const {
  normalizeLocalOptions,
  normalizeStandaloneOptions,
} = require("./options.js");

/**
 * Create a fully-local edge executor backed by fastembed-rs.
 * Models are downloaded from HuggingFace on first use and cached locally.
 * No network calls for inference — embedding runs on-device via ONNX.
 *
 * @param {string} dataDir
 * @param {boolean | {
 *   onDiskPayload?: boolean,
 *   model?: string,
 *   sparseModel?: string,
 *   multiModel?: string,
 *   imageModel?: string,
 *   rerankerModel?: string,
 *   cacheDir?: string,
 *   showDownloadProgress?: boolean,
 *   walSegmentMb?: number,
 *   bm25K1?: number,
 *   bm25B?: number,
 *   bm25AvgLen?: number,
 *   bm25Language?: string,
 *   bm25Tokenizer?: string,
 *   bm25Lowercase?: boolean,
 *   bm25AsciiFolding?: boolean,
 *   bm25MinTokenLen?: number,
 *   bm25MaxTokenLen?: number,
 * }} [options] - boolean is legacy `onDiskPayload`; object is preferred.
 * @returns {Client}
 *
 * @example
 *   const exec = localExecutor('./data');
 *   const exec = localExecutor('./data', false);
 *   const exec = localExecutor('./data', { model: 'AllMiniLML6V2', onDiskPayload: false });
 *   const exec = localExecutor('./data', { sparseModel: 'splade', rerankerModel: 'bge-reranker-base' });
 *   const exec = localExecutor('./data', { walSegmentMb: 8 });  // 8 MiB WAL segments
 */
function localExecutor(dataDir, options) {
  if (typeof dataDir !== 'string' || !dataDir) {
    throw new TypeError('localExecutor requires a non-empty dataDir string');
  }
  const opts = normalizeLocalOptions(options);
  const inner = callNative(() => nativeBinding.localExecutor(dataDir, opts));
  return new Client(inner);
}

/**
 * List dense ONNX models available for localExecutor({ model }).
 * @returns {Array<{ name: string, modelCode: string, dim: number, description: string }>}
 */
function listEmbeddingModels() {
  return callNative(() => nativeBinding.listEmbeddingModels());
}

/**
 * Create an edge executor that calls an external OpenAI-compatible embedding
 * endpoint.  Vector storage/search is still fully local — only the embedding
 * step goes over the network.
 *
 * @param {string}  dataDir      - path to the local Qdrant-compatible data directory
 * @param {string}  url          - full embeddings endpoint URL
 * @param {string}  embedKey     - Bearer token ("" for unauthenticated)
 * @param {string}  embedModel   - model name sent in the request body
 * @param {number}  embedDim     - output dimension
 * @param {boolean} [onDiskPayload] - store payloads on disk (default true)
 * @param {object}  [bm25Options] - local BM25 document encoder options
 *   (`bm25K1`/`bm25_k1`, `bm25B`/`bm25_b`, `bm25AvgLen`/`bm25_avg_len`,
 *   `bm25Language`, `bm25Tokenizer`, `bm25Lowercase`, `bm25AsciiFolding`,
 *   `bm25Stopwords`, `bm25Stemmer`, `bm25MinTokenLen`, `bm25MaxTokenLen`;
 *   snake_case aliases accepted). Values forward raw; invalid values fail
 *   closed in the native validator (`QQL-VALIDATION-CONFIG`).
 * @returns {Client}
 */
function httpExecutor(dataDir, url, embedKey, embedModel, embedDim, onDiskPayload, bm25Options) {
  if (typeof dataDir !== 'string' || !dataDir) {
    throw new TypeError('httpExecutor requires a non-empty dataDir string');
  }
  if (typeof url !== 'string' || !url) {
    throw new TypeError('httpExecutor requires a non-empty url string');
  }
  if (!Number.isSafeInteger(embedDim) || embedDim <= 0) {
    throw new TypeError('httpExecutor embedDim must be a positive integer');
  }
  if (
    bm25Options !== undefined &&
    (typeof bm25Options !== 'object' || bm25Options === null || Array.isArray(bm25Options))
  ) {
    throw new TypeError('httpExecutor bm25Options must be an object');
  }
  const bm25 = bm25Options ?? {};
  const inner = callNative(() =>
    nativeBinding.httpExecutor(
      dataDir,
      url,
      embedKey ?? '',
      embedModel ?? '',
      embedDim,
      onDiskPayload ?? true,
      {
        bm25K1: bm25.bm25K1 ?? bm25.bm25_k1,
        bm25B: bm25.bm25B ?? bm25.bm25_b,
        bm25AvgLen: bm25.bm25AvgLen ?? bm25.bm25_avg_len,
        bm25Language: bm25.bm25Language ?? bm25.bm25_language,
        bm25Tokenizer: bm25.bm25Tokenizer ?? bm25.bm25_tokenizer,
        bm25Lowercase: bm25.bm25Lowercase ?? bm25.bm25_lowercase,
        bm25AsciiFolding: bm25.bm25AsciiFolding ?? bm25.bm25_ascii_folding,
        bm25Stopwords: bm25.bm25Stopwords ?? bm25.bm25_stopwords,
        bm25Stemmer: bm25.bm25Stemmer ?? bm25.bm25_stemmer,
        bm25MinTokenLen: bm25.bm25MinTokenLen ?? bm25.bm25_min_token_len,
        bm25MaxTokenLen: bm25.bm25MaxTokenLen ?? bm25.bm25_max_token_len,
        bm25StopwordsLanguages:
          bm25.bm25StopwordsLanguages ?? bm25.bm25_stopwords_languages,
      },
    ),
  );
  return new Client(inner);
}

async function execute(query, options) {
  try {
    // Native returns the report as a live JS object (safe ints as Number,
    // snowflake u64 as BigInt) — never a JSON string, which would round
    // through JSON.parse.
    const raw = await nativeBinding.execute(
      normalizeQuery(query),
      normalizeStandaloneOptions(options),
    );
    return new ExecutionReport(raw);
  } catch (error) {
    throw buildError(error);
  }
}

async function executeHits(query, options, stmt = 0) {
  const report = await execute(query, options);
  return report.hits(stmt);
}

async function executeStmt(stmt, options) {
  try {
    return new ExecutionReport(
      await nativeBinding.executeStmt(
        stmt,
        normalizeStandaloneOptions(options),
      ),
    );
  } catch (error) {
    throw buildError(error);
  }
}

class Client {
  /**
   * Clients should be created via `localExecutor()` or `httpExecutor()`,
   * not by calling this constructor directly.
   * @param {object} inner - native JsClient handle
   */
  constructor(inner) {
    if (!inner || typeof inner.execute !== 'function') {
      throw new TypeError(
        'Client must be created via localExecutor() or httpExecutor()',
      );
    }
    this._inner = inner;
  }

  async execute(query, options) {
    try {
      const raw = await this._inner.execute(
        normalizeQuery(query),
        validateOptions(options) || undefined,
      );
      return new ExecutionReport(raw);
    } catch (error) {
      throw buildError(error);
    }
  }

  async executeHits(query, options, stmt = 0) {
    const report = await this.execute(query, options);
    return report.hits(stmt);
  }

  /**
   * Bulk ingest point objects (`{id, vector, …payload}`) in `batchSize`
   * chunks (default 100). One `:rows` template is prepared once with no
   * re-parse and no per-batch schema fetch. Vectors take plain arrays, packed
   * `Float32Array` / `Float64Array`, integer typed arrays for sparse
   * `indices`, or the flat `{data, dim}` multivector form (same contract as
   * Python and WASM; see the API surface bulk ingest contract).
   */
  async upsertMany(collection, rows, options) {
    try {
      if (
        options?.batchSize !== undefined &&
        (!Number.isInteger(options.batchSize) || options.batchSize < 1)
      ) {
        throw new TypeError("options.batchSize must be an integer >= 1");
      }
      const normalized = dx.normalizeUpsertRows(rows);
      const raw = await this._inner.upsertMany(
        collection,
        normalized,
        validateOptions(options) || undefined,
      );
      return new ExecutionReport(raw);
    } catch (error) {
      throw buildError(error);
    }
  }

  async close() {
    try {
      return await this._inner.close();
    } catch (error) {
      throw buildError(error);
    }
  }

  explain(query) {
    return callNative(() => this._inner.explain(query));
  }

  explainStmt(stmt) {
    return callNative(() => this._inner.explainStmt(stmt));
  }

  /**
   * Analyze a single query string or Stmt: static plan plus measured
   * execution (per-phase client timings, server time, hardware/inference
   * usage). Returns the AnalyzeReport as a plain object. Batches fail
   * closed — analyze each entry separately.
   */
  async explainAnalyze(query, options) {
    try {
      return await this._inner.explainAnalyze(
        normalizeQuery(query),
        validateOptions(options) || undefined,
      );
    } catch (error) {
      throw buildError(error);
    }
  }

  compile(query, params) {
    return callNative(() => this._inner.compile(query, params));
  }

  scrollCursor(collection, options) {
    return scrollCursor(this, collection, options);
  }

  scrollStream(collection, options) {
    return scrollStream(this, collection, options);
  }
}

/**
 * Substitute `:name` (object) or `?` (array) placeholders into a query string
 * or Stmt (shared implementation in dx-common.js, pyqql `bind` parity).
 */
function bind(query, params, options) {
  return dx.bind(nativeBinding, query, params, options);
}

module.exports = {
  parse,
  parseJson,
  isValid,
  injectFilter,
  tokenize,
  compileQuery,
  explain,
  explainStmt,
  bind,
  execute,
  executeHits,
  executeStmt,
  scrollCursor,
  scrollStream,
  localExecutor,
  listEmbeddingModels,
  httpExecutor,
  Client,
  Stmt: nativeBinding.Stmt,
  ScoredPoint,
  ExecutionReport,
  version: pkg.version,
  __version__: pkg.version,
};
