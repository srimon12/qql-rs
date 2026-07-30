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
      throw new Error('nqql-edge 0.1.1 does not provide a Linux musl binary');
    }
    return 'linux-x64-gnu';
  }
  if (platform === 'darwin' && arch === 'arm64') {
    return 'darwin-arm64';
  }
  if (platform === 'darwin' && arch === 'x64') {
    throw new Error(
      'nqql-edge 0.1.1 does not provide a macOS Intel binary because ONNX Runtime no longer ships the required x86_64 artifact',
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

function compileQuery(query) {
  return callNative(() => nativeBinding.compileQuery(query));
}

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
  if (typeof options === 'boolean') {
    return { onDiskPayload: options };
  }
  if (typeof options !== 'object' || Array.isArray(options)) {
    throw new TypeError(
      'localExecutor options must be a boolean (legacy onDiskPayload) or an object',
    );
  }
  return {
    onDiskPayload: options.onDiskPayload,
    model: typeof options.model === 'string' ? options.model : undefined,
    cacheDir: typeof options.cacheDir === 'string' ? options.cacheDir : undefined,
    showDownloadProgress:
      typeof options.showDownloadProgress === 'boolean'
        ? options.showDownloadProgress
        : undefined,
  };
}

/**
 * Create a fully-local edge executor backed by fastembed-rs.
 * Models are downloaded from HuggingFace on first use and cached locally.
 * No network calls for inference — embedding runs on-device via ONNX.
 *
 * @param {string} dataDir
 * @param {boolean | {
 *   onDiskPayload?: boolean,
 *   model?: string,
 *   cacheDir?: string,
 *   showDownloadProgress?: boolean,
 * }} [options] - boolean is legacy `onDiskPayload`; object is preferred.
 * @returns {Client}
 *
 * @example
 *   const exec = localExecutor('./data');
 *   const exec = localExecutor('./data', false);
 *   const exec = localExecutor('./data', { model: 'AllMiniLML6V2', onDiskPayload: false });
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

async function execute(query, options) {
  try {
    const raw = await nativeBinding.execute(
      normalizeQuery(query),
      normalizeStandaloneOptions(options),
    );
    return JSON.parse(raw);
  } catch (error) {
    throw buildError(error);
  }
}

async function executeStmt(stmt, options) {
  try {
    const raw = await nativeBinding.executeStmt(
      stmt,
      normalizeStandaloneOptions(options),
    );
    return JSON.parse(raw);
  } catch (error) {
    throw buildError(error);
  }
}

function normalizeStandaloneOptions(options) {
  if (options === undefined || options === null) {
    return undefined;
  }
  if (typeof options !== 'object' || Array.isArray(options)) {
    throw new TypeError('options must be an object');
  }
  return {
    dataDir: typeof options.dataDir === 'string' ? options.dataDir : './qdrant_data',
    onDiskPayload: options.onDiskPayload ?? true,
    model: typeof options.model === 'string' ? options.model : undefined,
    cacheDir: typeof options.cacheDir === 'string' ? options.cacheDir : undefined,
    showDownloadProgress:
      typeof options.showDownloadProgress === 'boolean'
        ? options.showDownloadProgress
        : undefined,
    embedUrl: typeof options.embedUrl === 'string' ? options.embedUrl : undefined,
    embedKey: typeof options.embedKey === 'string' ? options.embedKey : undefined,
    embedModel: typeof options.embedModel === 'string' ? options.embedModel : undefined,
    embedDim: Number.isSafeInteger(options.embedDim) ? options.embedDim : undefined,
    onError: options.onError,
  };
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
      return JSON.parse(raw);
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

  compile(query) {
    return callNative(() => this._inner.compile(query));
  }
}

const pkg = require('./package.json');

module.exports = {
  parse,
  parseJson,
  isValid,
  injectFilter,
    tokenize,
  compileQuery,
  explain,
  explainStmt,
  execute,
  executeStmt,
  localExecutor,
  listEmbeddingModels,
  httpExecutor,
  Client,
  Stmt: nativeBinding.Stmt,
  version: pkg.version,
  __version__: pkg.version,
};
