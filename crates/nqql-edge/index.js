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
  if (platform === 'linux' && arch === 'x64') {
    if (isMusl()) {
      throw new Error(`nqql-edge ${pkg.version} does not provide a Linux musl binary`);
    }
    return 'linux-x64-gnu';
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

if (nativeBinding.Stmt && !nativeBinding.Stmt.prototype.toJSON) {
  nativeBinding.Stmt.prototype.toJSON = function () {
    return this.toJson();
  };
}

function buildError(raw) {
  const message = raw instanceof Error ? raw.message : String(raw);
  try {
    const parsed = JSON.parse(message);
    if (parsed && parsed.code) {
      const error = new Error(parsed.message || message);
      error.code = parsed.code;
      error.kind = parsed.kind;
      error.span = parsed.span ?? null;
      return error;
    }
  } catch (_) {
    // Non-QQL errors retain their original JS error and stack.
  }
  return raw instanceof Error ? raw : new Error(message);
}

function callNative(call) {
  try {
    return call();
  } catch (error) {
    throw buildError(error);
  }
}

function normalizeQuery(query) {
  if (query instanceof nativeBinding.Stmt) {
    return query.toObject();
  }
  if (Array.isArray(query)) {
    return query.map(normalizeQuery);
  }
  return query;
}

function validateOptions(options) {
  if (
    options !== undefined &&
    options !== null &&
    (typeof options !== 'object' || Array.isArray(options))
  ) {
    throw new TypeError('options must be an object');
  }
  const value = options?.onError;
  if (value !== undefined && value !== 'stop' && value !== 'continue') {
    throw new TypeError("options.onError must be 'stop' or 'continue'");
  }
  return options;
}

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
 * }} [options] - boolean is legacy `onDiskPayload`; object is preferred.
 * @returns {Client}
 *
 * @example
 *   const exec = localExecutor('./data');
 *   const exec = localExecutor('./data', false);
 *   const exec = localExecutor('./data', { model: 'AllMiniLML6V2', onDiskPayload: false });
 *   const exec = localExecutor('./data', { sparseModel: 'splade', rerankerModel: 'bge-reranker-base' });
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
 * @returns {Client}
 */
function httpExecutor(dataDir, url, embedKey, embedModel, embedDim, onDiskPayload) {
  if (typeof dataDir !== 'string' || !dataDir) {
    throw new TypeError('httpExecutor requires a non-empty dataDir string');
  }
  if (typeof url !== 'string' || !url) {
    throw new TypeError('httpExecutor requires a non-empty url string');
  }
  if (!Number.isSafeInteger(embedDim) || embedDim <= 0) {
    throw new TypeError('httpExecutor embedDim must be a positive integer');
  }
  const inner = callNative(() =>
    nativeBinding.httpExecutor(
      dataDir,
      url,
      embedKey ?? '',
      embedModel ?? '',
      embedDim,
      onDiskPayload ?? true,
    ),
  );
  return new Client(inner);
}

class ScoredPoint {
  constructor(data) {
    if (!data || typeof data !== 'object') {
      throw new TypeError('ScoredPoint requires a hit object');
    }
    // Field defaults mirror pyqql's ScoredPoint dataclass; Object.assign
    // below overlays the hit's own values.
    this.id = data.id;
    this.score = data.score ?? 0;
    this.payload = data.payload ?? null;
    this.text = data.text ?? null;
    this.collection = data.collection ?? null;
    Object.assign(this, data);
  }

  get(key, defaultValue = null) {
    if (this.payload && typeof this.payload === 'object' && key in this.payload) {
      return this.payload[key];
    }
    return defaultValue;
  }
}

class ExecutionReport {
  constructor(data) {
    // Defaults mirror pyqql's ExecutionReport dict subclass.
    this.ok = false;
    this.results = [];
    this.succeeded = 0;
    this.failed = 0;
    Object.assign(this, data);
  }

  #resultAt(stmt) {
    const res = this.results;
    if (!Array.isArray(res) || res.length === 0) return undefined;
    const idx = stmt < 0 ? res.length + stmt : stmt;
    return res[idx];
  }

  hits(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res || !Array.isArray(res.data)) return [];
    return res.data
      .filter((d) => d && typeof d === 'object')
      .map((d) => new ScoredPoint(d));
  }

  points(stmt = 0) {
    return this.hits(stmt);
  }

  facet(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res) return [];
    if (Array.isArray(res.data)) return res.data;
    if (typeof res.data === 'object' && res.data !== null) {
      return res.data.result?.hits || res.data.hits || [];
    }
    return [];
  }

  count(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res) return 0;
    if (typeof res.data === 'number') return res.data;
    if (typeof res.data === 'object' && res.data !== null) {
      const c = res.data.result?.count ?? res.data.count;
      if (typeof c === 'number') return c;
    }
    if (typeof res.message === 'string' && res.message.startsWith('Count: ')) {
      const parsed = parseInt(res.message.slice(7), 10);
      if (!Number.isNaN(parsed)) return parsed;
    }
    return 0;
  }
}

async function execute(query, options) {
  try {
    const raw = await nativeBinding.execute(
      normalizeQuery(query),
      normalizeStandaloneOptions(options),
    );
    return new ExecutionReport(JSON.parse(raw));
  } catch (error) {
    throw buildError(error);
  }
}

async function executeHits(query, options) {
  const report = await execute(query, options);
  return report.hits(0);
}

async function executeStmt(stmt, options) {
  try {
    const raw = await nativeBinding.executeStmt(
      stmt,
      normalizeStandaloneOptions(options),
    );
    return new ExecutionReport(JSON.parse(raw));
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
      return new ExecutionReport(JSON.parse(raw));
    } catch (error) {
      throw buildError(error);
    }
  }

  async executeHits(query, options) {
    const report = await this.execute(query, options);
    return report.hits(0);
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

  compile(query, params) {
    return callNative(() => this._inner.compile(query, params));
  }
}

/**
 * Substitute `:name` (object) or `?` (array) placeholders into a query string
 * or Stmt. Mirrors pyqql's `bind`: Stmt inputs return a bound Stmt (or, with
 * `truncateVectors`, the truncated readable string); string inputs return the
 * bound query string. Without `params`, the input is returned unchanged.
 */
function bind(query, params, options) {
  if (query instanceof nativeBinding.Stmt) {
    const bound = query.bind(params ?? undefined);
    const truncate = options?.truncateVectors ?? false;
    return truncate ? bound.toReadableString() : bound;
  }
  return callNative(() => nativeBinding.bind(query, params ?? undefined, options));
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
