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

const { normalizeClientOptions } = require('./options.js');

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
 * or Stmt. Mirrors pyqql's `bind`: Stmt inputs return a bound Stmt (or, with
 * `truncateVectors`, the truncated readable string); string inputs return the
 * bound query string. Without `params`, the input is returned unchanged.
 */
function bind(query, params, options) {
  if (query instanceof nativeBinding.Stmt) {
    const bound = query.bind(params ?? undefined);
    const truncate = options?.truncateVectors ?? options?.truncate_vectors ?? false;
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
  Client,
  Stmt: nativeBinding.Stmt,
  ScoredPoint,
  ExecutionReport,
  HttpEmbedder,
  version: pkg.version,
  __version__: pkg.version,
};
