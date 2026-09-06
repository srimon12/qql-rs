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
      throw new Error(`nqql ${pkg.version} does not provide a Linux musl binary`);
    }
    return 'linux-x64-gnu';
  }
  if (platform === 'darwin' && (arch === 'x64' || arch === 'arm64')) {
    return `darwin-${arch}`;
  }
  if (platform === 'win32' && arch === 'x64') {
    return 'win32-x64-msvc';
  }
  throw new Error(`nqql does not provide a native binary for ${platform}-${arch}`);
}

const target = nativeTarget();
let nativeBinding;
try {
  nativeBinding = require(`./index.${target}.node`);
} catch (localError) {
  try {
    nativeBinding = require(`@veristamp/nqql-${target}`);
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

const { buildError, callNative, validateOptions, ScoredPoint, ExecutionReport } = dx;

const normalizeQuery = (query) => dx.normalizeQuery(nativeBinding.Stmt, query);



const { normalizeClientOptions } = require('./options.js');


class HttpEmbedder {
  constructor(options) {
    if (!options || typeof options.endpoint !== 'string' || !options.endpoint) {
      throw new TypeError('HttpEmbedder requires a non-empty endpoint');
    }
    if (typeof options.model !== 'string' || !options.model) {
      throw new TypeError('HttpEmbedder requires a non-empty model');
    }
    if (!Number.isSafeInteger(options.dimension) || options.dimension <= 0) {
      throw new TypeError('HttpEmbedder dimension must be a positive integer');
    }
    const apiKey = options.apiKey ?? options.api_key;
    if (apiKey !== undefined && typeof apiKey !== 'string') {
      throw new TypeError('HttpEmbedder apiKey must be a string');
    }
    for (const key of ['multiApiKey', 'imageApiKey']) {
      const value = options[key] ?? options[key === 'multiApiKey' ? 'multi_api_key' : 'image_api_key'];
      if (value !== undefined && typeof value !== 'string') {
        throw new TypeError(`HttpEmbedder ${key} must be a string`);
      }
    }
    for (const key of ['multiDimension', 'imageDimension']) {
      const value = options[key] ?? options[key === 'multiDimension' ? 'multi_dimension' : 'image_dimension'];
      if (value !== undefined && (!Number.isSafeInteger(value) || value <= 0)) {
        throw new TypeError(`HttpEmbedder ${key} must be a positive integer`);
      }
    }
    this.endpoint = options.endpoint;
    this.apiKey = apiKey ?? '';
    this.model = options.model;
    this.dimension = options.dimension;
    this.multiEndpoint = options.multiEndpoint ?? options.multi_endpoint ?? '';
    this.multiApiKey = options.multiApiKey ?? options.multi_api_key ?? '';
    this.multiModel = options.multiModel ?? options.multi_model ?? '';
    this.multiDimension = options.multiDimension ?? options.multi_dimension ?? null;
    this.imageEndpoint = options.imageEndpoint ?? options.image_endpoint ?? '';
    this.imageApiKey = options.imageApiKey ?? options.image_api_key ?? '';
    this.imageModel = options.imageModel ?? options.image_model ?? '';
    this.imageDimension = options.imageDimension ?? options.image_dimension ?? null;
    this.rerankEndpoint = options.rerankEndpoint ?? options.rerank_endpoint ?? '';
    this.rerankApiKey = options.rerankApiKey ?? options.rerank_api_key ?? '';
    this.rerankModel = options.rerankModel ?? options.rerank_model ?? '';
  }
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

async function execute(query, options) {
  try {
    const raw = await nativeBinding.execute(
      normalizeQuery(query),
      normalizeClientOptions(validateOptions(options)),
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
    return new ExecutionReport(
      JSON.parse(
        await nativeBinding.executeStmt(
          stmt,
          normalizeClientOptions(validateOptions(options)),
        ),
      ),
    );
  } catch (error) {
    throw buildError(error);
  }
}

class Client {
  constructor(options) {
    const normalized = normalizeClientOptions(options);
    this._inner = new nativeBinding.Client(normalized);
    this._routeAffinity = normalized?.routeAffinity || null;
  }

  /** Qdrant 1.19+ read affinity key set at construction; `null` when unset. */
  get routeAffinity() {
    return this._routeAffinity;
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

  explain(query) {
    return callNative(() => this._inner.explain(query));
  }

  explainStmt(stmt) {
    return callNative(() => this._inner.explainStmt(stmt));
  }

  compile(query, params) {
    return callNative(() => this._inner.compile(query, params));
  }

  async close() {
    if (typeof this._inner.close === "function") {
      await this._inner.close();
    }
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
  Client,
  Stmt: nativeBinding.Stmt,
  ScoredPoint,
  ExecutionReport,
  HttpEmbedder,
  version: pkg.version,
  __version__: pkg.version,
};
