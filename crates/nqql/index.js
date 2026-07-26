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
      throw new Error('nqql 0.1.0 does not provide a Linux musl binary');
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
    nativeBinding = require(`nqql-${target}`);
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

function normalizeClientOptions(options) {
  if (!options) {
    return undefined;
  }
  if (typeof options !== 'object' || Array.isArray(options)) {
    throw new TypeError('client options must be an object');
  }
  return {
    ...options,
    embedder: options.embedder
      ? {
          endpoint: options.embedder.endpoint,
          apiKey: options.embedder.apiKey,
          model: options.embedder.model,
          dimension: options.embedder.dimension,
        }
      : undefined,
  };
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
    if (options.apiKey !== undefined && typeof options.apiKey !== 'string') {
      throw new TypeError('HttpEmbedder apiKey must be a string');
    }
    this.endpoint = options.endpoint;
    this.apiKey = options.apiKey ?? '';
    this.model = options.model;
    this.dimension = options.dimension;
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

function compileQuery(query) {
  return callNative(() => nativeBinding.compileQuery(query));
}

async function execute(query, options) {
  try {
    const raw = await nativeBinding.execute(
      normalizeQuery(query),
      normalizeClientOptions(validateOptions(options)),
    );
    return JSON.parse(raw);
  } catch (error) {
    throw buildError(error);
  }
}

async function executeStmt(stmt, options) {
  try {
    return JSON.parse(
      await nativeBinding.executeStmt(stmt, normalizeClientOptions(options)),
    );
  } catch (error) {
    throw buildError(error);
  }
}

class Client {
  constructor(options) {
    this._inner = new nativeBinding.Client(normalizeClientOptions(options));
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
  Client,
  Stmt: nativeBinding.Stmt,
  HttpEmbedder,
};
