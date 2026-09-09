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
 * Install the `toJSON` alias and error-mapping wrappers on the native Stmt
 * prototype so method errors surface with `.code` / `.kind` / `.span`.
 */
function installStmtToJSON(Stmt) {
  if (!Stmt || !Stmt.prototype) return;
  if (!Stmt.prototype.toJSON) {
    Stmt.prototype.toJSON = function () {
      return this.toJson();
    };
  }
  const origBind = Stmt.prototype.bind;
  if (origBind && !origBind._wrapped) {
    Stmt.prototype.bind = function (params) {
      return callNative(() => origBind.call(this, params));
    };
    Stmt.prototype.bind._wrapped = true;
  }
  const origCompileRoute = Stmt.prototype.compileRoute;
  if (origCompileRoute && !origCompileRoute._wrapped) {
    Stmt.prototype.compileRoute = function (params) {
      return callNative(() => origCompileRoute.call(this, params));
    };
    Stmt.prototype.compileRoute._wrapped = true;
  }
  const origInjectFilter = Stmt.prototype.injectFilter;
  if (origInjectFilter && !origInjectFilter._wrapped) {
    Stmt.prototype.injectFilter = function (field, op, value) {
      return callNative(() => origInjectFilter.call(this, field, op, value));
    };
    Stmt.prototype.injectFilter._wrapped = true;
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
    this.vector = data.vector ?? null;
    this.shard_key = data.shard_key ?? null;
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
      // Only map entries shaped like points; facet entries
      // ({ value, count }) are not points.
      .filter((d) => d && typeof d === 'object' && 'id' in d)
      .map((d) => new ScoredPoint(d));
  }

  points(stmt = 0) {
    return this.hits(stmt);
  }

  ids(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res) return [];
    let items = [];
    if (Array.isArray(res.data)) {
      items = res.data;
    } else if (typeof res.data === 'object' && res.data !== null) {
      const raw = res.data.result;
      if (raw && typeof raw === 'object') {
        items = Array.isArray(raw) ? raw : (raw.points || raw.hits || []);
      } else {
        items = res.data.points || res.data.hits || [];
      }
    }
    const out = [];
    for (const item of items) {
      if (item && typeof item === 'object' && 'id' in item) {
        out.push(item.id);
      }
    }
    return out;
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

/** Default page size for {@link scrollCursor} when `batchSize` is unset. */
const SCROLL_DEFAULT_BATCH_SIZE = 100;

/**
 * Validate `scrollCursor` / `scrollStream` arguments without touching the
 * network. Returns the normalized `{ batchSize, where, withPayload,
 * withVector }` tuple; throws `TypeError` on programmer errors so bad
 * options fail fast instead of mid-iteration.
 */
function resolveScrollArgs(client, collection, options) {
  if (!client || typeof client.execute !== 'function') {
    throw new TypeError('scrollCursor requires a client with an execute() method');
  }
  if (typeof collection !== 'string' || !collection) {
    throw new TypeError('scrollCursor requires a non-empty collection name string');
  }
  if (
    options !== undefined &&
    options !== null &&
    (typeof options !== 'object' || Array.isArray(options))
  ) {
    throw new TypeError('scrollCursor options must be an object');
  }
  const batchSize = options?.batchSize ?? SCROLL_DEFAULT_BATCH_SIZE;
  if (!Number.isInteger(batchSize) || batchSize < 1) {
    throw new TypeError('options.batchSize must be an integer >= 1');
  }
  const where = options?.where ?? '';
  if (typeof where !== 'string') {
    throw new TypeError('options.where must be a QQL filter string');
  }
  const withPayload = options?.withPayload ?? true;
  if (typeof withPayload !== 'boolean') {
    throw new TypeError('options.withPayload must be a boolean');
  }
  const withVector = options?.withVector ?? false;
  if (typeof withVector !== 'boolean') {
    throw new TypeError('options.withVector must be a boolean');
  }
  const shardKey = options?.shardKey;
  if (
    shardKey !== undefined &&
    typeof shardKey !== 'string' &&
    typeof shardKey !== 'number' &&
    typeof shardKey !== 'bigint'
  ) {
    throw new TypeError('options.shardKey must be a string, number, or bigint');
  }
  const params = options?.params;
  if (params !== undefined) {
    if (typeof params !== 'object' || params === null || Array.isArray(params)) {
      throw new TypeError('options.params must be an object with named parameters');
    }
    if (Object.prototype.hasOwnProperty.call(params, 'cursor')) {
      throw new TypeError('options.params cannot contain reserved parameter "cursor"');
    }
  }
  return { batchSize, where: where.trim(), params, withPayload, withVector, shardKey };
}

function formatCollection(name) {
  if (typeof name !== 'string') return name;
  if (/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name)) {
    return name;
  }
  if ((name.startsWith('"') && name.endsWith('"')) || (name.startsWith("'") && name.endsWith("'"))) {
    return name;
  }
  return `"${name.replace(/"/g, '""')}"`;
}

/**
 * Render one SCROLL page statement. Clause order follows the grammar
 * (`parse_scroll`): WHERE → AFTER → SHARD → WITH VECTOR → LIMIT. The cursor always
 * binds as `:cursor` (AFTER accepts named point-ID params); the first page
 * omits AFTER entirely. `WITH VECTOR` is appended only when requested —
 * SCROLL omits vectors by default, and payloads are included by default so
 * no clause is emitted for them. There is deliberately no `WITH PAYLOAD`
 * spelling here: SCROLL has no payload selector in the grammar (it is a
 * parse error; the planner always fetches payload), so `withPayload: false`
 * is applied client-side by nulling `payload` before yielding.
 */
function buildScrollStatement(collection, resolved, cursor) {
  let sql = `SCROLL FROM ${formatCollection(collection)}`;
  if (resolved.where) {
    sql += ` WHERE ${resolved.where}`;
  }
  let params = resolved.params ? { ...resolved.params } : undefined;
  if (cursor !== undefined) {
    sql += ' AFTER :cursor';
    params = params ? { ...params, cursor } : { cursor };
  }
  if (resolved.shardKey !== undefined) {
    if (typeof resolved.shardKey === 'number' || typeof resolved.shardKey === 'bigint') {
      sql += ` SHARD ${resolved.shardKey}`;
    } else {
      sql += ` SHARD '${String(resolved.shardKey).replace(/'/g, "''")}'`;
    }
  }
  if (resolved.withVector) {
    sql += ' WITH VECTOR';
  }
  sql += ` LIMIT ${resolved.batchSize}`;
  return { sql, params };
}

/**
 * Lazily page through a collection with `SCROLL`, yielding individual
 * `ScoredPoint`s. Pages fetch one at a time via the existing `execute`
 * path — at most one page is ever buffered, so memory stays bounded no
 * matter how large the collection is. Iteration stops on the first empty
 * page. Works with any client exposing `execute(sql, { params })`
 * (server `Client` and edge `Client` alike).
 */
async function* scrollCursor(client, collection, options) {
  const resolved = resolveScrollArgs(client, collection, options);
  let cursor;
  for (;;) {
    const { sql, params } = buildScrollStatement(collection, resolved, cursor);
    const report = await client.execute(
      sql,
      params === undefined ? undefined : { params },
    );
    const hits = report && typeof report.hits === 'function' ? report.hits() : [];
    if (hits.length === 0) {
      return;
    }
    const nextCursor = hits[hits.length - 1]?.id;
    if (cursor !== undefined && nextCursor === cursor) {
      return;
    }
    for (const hit of hits) {
      if (resolved.withPayload) {
        yield hit;
      } else {
        yield new ScoredPoint({ ...hit, payload: null });
      }
    }
    cursor = nextCursor;
  }
}

/**
 * WHATWG stream over {@link scrollCursor}. Prefers `ReadableStream.from`;
 * environments on the engines floor (Node >= 18) predate
 * `ReadableStream.from`, so fall back to a pull-driven `ReadableStream`
 * wrapper — one iterator advance per pull, which preserves the same
 * backpressure (a slow reader never buffers more than the stream's
 * internal queue). Throws clearly when no global `ReadableStream` exists
 * at all. Programmer errors (bad client/collection/options) throw
 * synchronously instead of surfacing on the first pull.
 */
function scrollStream(client, collection, options) {
  resolveScrollArgs(client, collection, options);
  const iterator = scrollCursor(client, collection, options);
  const RS = typeof ReadableStream === 'function' ? ReadableStream : undefined;
  if (RS === undefined) {
    throw new Error('scrollStream requires a global ReadableStream (Node >= 18)');
  }
  if (typeof RS.from === 'function') {
    return RS.from(iterator);
  }
  return new RS({
    async pull(controller) {
      const step = await iterator.next();
      if (step.done) {
        controller.close();
      } else {
        controller.enqueue(step.value);
      }
    },
    async cancel() {
      await iterator.return();
    },
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
  resolveScrollArgs,
  buildScrollStatement,
  scrollCursor,
  scrollStream,
};
