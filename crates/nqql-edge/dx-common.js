"use strict";

/**
 * Shared DX layer for the @veristamp/nqql and @veristamp/nqql-edge wrappers:
 * error mapping, the typed ExecutionReport / ScoredPoint classes, and the
 * Stmt-aware module bind.
 *
 * BYTE-IDENTICAL in crates/nqql and crates/nqql-edge — a CI check diffs the
 * two copies, so edit both or neither. They must stay in lockstep with the
 * Python `pyqql/pyqql/_dx_report.py` classes (same report contract).
 */

/**
 * Install the `toJSON` alias on the native Stmt prototype (JSON.stringify
 * support) exactly once.
 */
function installStmtToJSON(Stmt) {
  if (Stmt && !Stmt.prototype.toJSON) {
    Stmt.prototype.toJSON = function () {
      return this.toJson();
    };
  }
}

/**
 * Rebuild a JS error from a serialized QqlError payload so hosts get
 * `.code` / `.kind` / `.span` alongside the message.
 */
function buildError(raw) {
  const message = raw instanceof Error ? raw.message : String(raw);
  try {
    const parsed = JSON.parse(message);
    if (parsed && parsed.code) {
      const error = new Error(parsed.message || message);
      error.code = parsed.code;
      error.kind = parsed.kind;
      error.span = parsed.span ?? null;
      // Structured fields (url, status, request_id, …) arrive as a
      // Vec<{key, value}> from serde; normalize to a map + a `request_id`
      // convenience attribute, mirroring the Python SDK's error surface.
      error.fields = {};
      if (Array.isArray(parsed.fields)) {
        for (const f of parsed.fields) {
          if (f && f.key !== undefined) error.fields[f.key] = f.value;
        }
      }
      if (error.fields.request_id !== undefined) {
        error.request_id = error.fields.request_id;
      }
      return error;
    }
  } catch (_) {
    // Non-QQL errors retain their original JS error and stack.
  }
  return raw instanceof Error ? raw : new Error(message);
}

/** Invoke a native entry point, mapping QQL errors through buildError. */
function callNative(call) {
  try {
    return call();
  } catch (error) {
    throw buildError(error);
  }
}

/** Normalize a query argument (Stmt → AST object; arrays mapped) for native. */
function normalizeQuery(Stmt, query) {
  if (query instanceof Stmt) {
    return query.toObject();
  }
  if (Array.isArray(query)) {
    return query.map((q) => normalizeQuery(Stmt, q));
  }
  return query;
}

/** Validate shared execute options (`onError` enum; object-ness). */
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

  groups(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res || typeof res.data !== 'object' || res.data === null) return [];
    const raw = res.data.result;
    const nested = raw && typeof raw === 'object' ? raw.groups : undefined;
    const groups = nested !== undefined && nested !== null ? nested : res.data.groups;
    return Array.isArray(groups) ? groups : [];
  }
}

/**
 * Module-level `bind`: substitute `:name` (object) or `?` (array)
 * placeholders into a query string or Stmt. Mirrors pyqql's `bind`: Stmt
 * inputs return a bound Stmt (or, with `truncateVectors`, the truncated
 * readable string); string inputs return the bound query string. Without
 * `params`, the input is returned unchanged.
 */
function bind(native, query, params, options) {
  return callNative(() => {
    if (query instanceof native.Stmt) {
      const bound = query.bind(params ?? undefined);
      const truncate = options?.truncateVectors ?? options?.truncate_vectors ?? false;
      return truncate ? bound.toReadableString() : bound;
    }
    return native.bind(query, params ?? undefined, options);
  });
}

module.exports = {
  installStmtToJSON,
  buildError,
  callNative,
  normalizeQuery,
  validateOptions,
  ScoredPoint,
  ExecutionReport,
  bind,
};
