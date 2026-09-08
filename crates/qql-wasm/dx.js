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

export class ExecutionReport {
  constructor(data) {
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
