/**
 * Typed DX layer for qql-wasm: ScoredPoint, ExecutionReport, error mapping.
 * Aligned with pyqql's ExecutionReport and nqql's dx-common.js.
 */

export class ScoredPoint {
  constructor(data) {
    if (!data || typeof data !== 'object') {
      throw new TypeError('ScoredPoint requires a hit object');
    }
    this.id = data.id;
    this.score = data.score ?? 0;
    this.payload = data.payload ?? null;
    // `text` is derived from the payload's `text` field: the typed hit has
    // no separate text field.
    this.text = data.payload?.text ?? null;
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

function aggregateTelemetry(results) {
  let timeSum = null;
  let usage = null;
  let any = false;
  for (const res of results) {
    const tel = res && typeof res === 'object' ? res.telemetry : null;
    if (!tel || typeof tel !== 'object') continue;
    const hasTime = typeof tel.time_s === 'number';
    const hasUsage = tel.usage !== undefined && tel.usage !== null;
    if (!hasTime && !hasUsage) continue;
    any = true;
    if (hasTime) {
      timeSum = (timeSum ?? 0) + tel.time_s;
    }
    if (hasUsage) {
      usage = usage === null ? tel.usage : mergeUsage(usage, tel.usage);
    }
  }
  if (!any) return null;
  const out = {};
  if (timeSum !== null) out.time_s = timeSum;
  if (usage !== null) out.usage = usage;
  return out;
}

function mergeUsage(a, b) {
  if (!a || typeof a !== 'object') return b;
  if (!b || typeof b !== 'object') return a;
  const out = { ...a, ...b };
  if (a.hardware && b.hardware && typeof a.hardware === 'object' && typeof b.hardware === 'object') {
    const fields = [
      'cpu',
      'payload_io_read',
      'payload_io_write',
      'payload_index_io_read',
      'payload_index_io_write',
      'vector_io_read',
      'vector_io_write',
    ];
    const hardware = {};
    for (const field of fields) {
      const av = typeof a.hardware[field] === 'number' ? a.hardware[field] : 0;
      const bv = typeof b.hardware[field] === 'number' ? b.hardware[field] : 0;
      hardware[field] = av + bv;
    }
    out.hardware = hardware;
  }
  if (a.inference && b.inference && typeof a.inference === 'object' && typeof b.inference === 'object') {
    const models = { ...(a.inference.models ?? {}), ...(b.inference.models ?? {}) };
    for (const name of Object.keys(b.inference.models ?? {})) {
      const av = a.inference.models?.[name]?.tokens ?? 0;
      const bv = b.inference.models?.[name]?.tokens ?? 0;
      models[name] = { tokens: av + bv };
    }
    out.inference = { models };
  }
  return out;
}

const POINT_OPS = new Set(['QUERY', 'GET_POINTS', 'SCROLL', 'CROSS_RERANK']);

export class ExecutionReport {
  constructor(data) {
    this.ok = false;
    this.results = [];
    this.succeeded = 0;
    this.failed = 0;
    this._telemetry = null;
    Object.assign(this, data);
    // When the plain report carries no aggregated telemetry but per-result
    // telemetry exists (older payloads), aggregate here so readers always use
    // `report.telemetry` instead of hand-rolling the sum.
    if ((this._telemetry === null || this._telemetry === undefined) && Array.isArray(this.results)) {
      const aggregated = aggregateTelemetry(this.results);
      if (aggregated !== null) {
        this._telemetry = aggregated;
      }
    }
  }

  get telemetry() {
    return this._telemetry ?? null;
  }

  set telemetry(value) {
    this._telemetry = value ?? null;
  }

  #resultAt(stmt) {
    const res = this.results;
    if (!Array.isArray(res) || res.length === 0) return undefined;
    const idx = stmt < 0 ? res.length + stmt : stmt;
    return res[idx];
  }

  hits(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res || !POINT_OPS.has(res.operation) || !Array.isArray(res.data)) return [];
    return res.data.map((hit) => new ScoredPoint(hit));
  }

  points(stmt = 0) {
    return this.hits(stmt);
  }

  ids(stmt = 0) {
    return this.hits(stmt).map((hit) => hit.id);
  }

  facet(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res || res.operation !== 'FACET' || !Array.isArray(res.data)) return [];
    return res.data;
  }

  count(stmt = 0) {
    const res = this.#resultAt(stmt);
    const data = res?.data;
    // Count / mutation payloads are `{count: n}`; status-only mutations
    // serialize `null`.
    if (!data || typeof data !== 'object' || Array.isArray(data)) return 0;
    return typeof data.count === 'number' ? data.count : 0;
  }

  groups(stmt = 0) {
    const res = this.#resultAt(stmt);
    if (!res || res.operation !== 'QUERY_GROUPS') return [];
    const groups = res.data?.groups;
    return Array.isArray(groups) ? groups : [];
  }
}

export function buildError(raw) {
  const message = raw instanceof Error ? raw.message : String(raw);
  try {
    const parsed = JSON.parse(message);
    if (parsed && parsed.code) {
      const error = new Error(parsed.message || message);
      error.code = parsed.code;
      error.kind = parsed.kind;
      error.span = parsed.span ?? null;
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
  } catch (_) {}
  return raw instanceof Error ? raw : new Error(message);
}

export function wrapReport(data) {
  if (data instanceof ExecutionReport) return data;
  return new ExecutionReport(data);
}

export async function executeHits(client, query, options) {
  const report = wrapReport(await client.execute(query, options));
  return report.hits(0);
}

const SCROLL_DEFAULT_BATCH_SIZE = 100;

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

export async function* scrollCursor(client, collection, options) {
  const resolved = resolveScrollArgs(client, collection, options);
  let cursor;
  for (;;) {
    const { sql, params } = buildScrollStatement(collection, resolved, cursor);
    const raw = await client.execute(
      sql,
      params === undefined ? undefined : { params },
    );
    const report = wrapReport(raw);
    const hits = report.hits();
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
