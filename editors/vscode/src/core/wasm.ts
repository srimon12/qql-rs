/**
 * WASM parser bridge — imports the bundled qql-wasm Node.js package.
 *
 * The Node.js wasm-pack target loads the .wasm binary synchronously;
 * no async init() is required.
 */

import type { QqlParams } from "./params";
import type { CompiledRoute, WasmAnalyzeResult } from "./types";

export type { CompiledRoute, QqlParams, WasmAnalyzeResult };

let _analyze: ((input: string) => WasmAnalyzeResult) | null = null;
let _explain: ((input: string) => string) | null = null;
let _compile: ((query: string, params?: unknown) => CompiledRoute) | null = null;
let _bind: ((query: string, params?: unknown, options?: unknown) => string) | null = null;
let _Stmt:
  | (new (
      input: string
    ) => {
      injectFilter(field: string, op: string, value: unknown): void;
      toString(): string;
    })
  | null = null;
let _tokenize: ((input: string) => unknown[]) | null = null;
let _isValid: ((input: string) => boolean) | null = null;
let _parse: ((input: string) => unknown[]) | null = null;
let _format: ((input: string) => string) | null = null;

export function initWasm(): void {
  if (_analyze) return;

  try {
    // Compiled output lives at out/core/wasm.js → ../../wasm/qql_wasm.js
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const path = require("node:path") as typeof import("path");
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const qqlWasm = require(path.join(__dirname, "..", "..", "wasm", "qql_wasm.js"));
    if (typeof qqlWasm.analyze !== "function") {
      throw new Error("qql-wasm module loaded but analyze() is not a function");
    }
    _analyze = qqlWasm.analyze as (input: string) => WasmAnalyzeResult;
    _explain = typeof qqlWasm.explain === "function" ? qqlWasm.explain : null;
    _compile =
      typeof qqlWasm.compile === "function"
        ? (qqlWasm.compile as (query: string, params?: unknown) => CompiledRoute)
        : null;
    _bind = typeof qqlWasm.bind === "function" ? qqlWasm.bind : null;
    _Stmt = typeof qqlWasm.Stmt === "function" ? qqlWasm.Stmt : null;
    _tokenize = typeof qqlWasm.tokenize === "function" ? qqlWasm.tokenize : null;
    _isValid = typeof qqlWasm.isValid === "function" ? qqlWasm.isValid : null;
    _parse = typeof qqlWasm.parse === "function" ? qqlWasm.parse : null;
    _format = typeof qqlWasm.formatQuery === "function" ? qqlWasm.formatQuery : null;
  } catch (err) {
    throw new Error(
      `Failed to load QQL WASM parser: ${err instanceof Error ? err.message : String(err)}`
    );
  }
}

export function isWasmReady(): boolean {
  return _analyze !== null;
}

const EMPTY: WasmAnalyzeResult = {
  valid: false,
  statements_count: 0,
  tokens: [],
  ast: null,
  route: null,
  routes: [],
  explain: null,
  error: null,
};

export function analyzeQql(source: string, params?: QqlParams | null): WasmAnalyzeResult {
  if (!_analyze) {
    throw new Error("WASM parser not initialized — call initWasm() first");
  }
  if (!source.trim()) {
    return EMPTY;
  }
  const raw = _analyze(source);
  // Raw-first: valid documents (and non-bind errors) never pay for a second
  // pass and keep exact byte offsets. Only unbound `:name` / `?` placeholders
  // retry against the bound text — without params there is nothing to retry.
  if (raw.valid || params == null || _bind == null || !isBindError(raw.error?.code)) {
    return raw;
  }
  try {
    const bound = _bind(source, params);
    if (bound === source) return raw;
    return _analyze(bound);
  } catch {
    return raw;
  }
}

/** Substitute `:name` / `?` placeholders; returns `query` unchanged without params. */
export function bindQql(query: string, params?: QqlParams | null): string {
  if (params == null || _bind == null) return query;
  return _bind(query, params);
}

/**
 * Inject `field op value` into one statement and return canonical QQL.
 * Fail-closed: unsupported statements/operators throw with the WASM message.
 */
export function injectFilterQql(source: string, field: string, op: string, value: unknown): string {
  if (_Stmt == null) {
    throw new Error("injectFilter() is not available from the WASM module");
  }
  const stmt = new _Stmt(source);
  stmt.injectFilter(field, op, value);
  return stmt.toString();
}

function isBindError(code: string | null | undefined): boolean {
  return code?.startsWith("QQL-BIND-") ?? false;
}

/**
 * Explain one or more statements.
 *
 * Prefer analyze() so multi-statement scripts and single statements share the
 * same path. The raw wasm `explain()` only accepts a single statement.
 */
export function explainQql(source: string, params?: QqlParams | null): string {
  const trimmed = source.trim();
  if (!trimmed) {
    throw new Error("Nothing to explain — empty selection");
  }

  // analyze() returns explain_nodes for the full script (1..N statements)
  const result = analyzeQql(trimmed, params);
  if (result.explain) {
    return result.explain;
  }
  if (result.error) {
    throw new Error(`${result.error.code}: ${result.error.message}`);
  }

  // Fallback if analyze omitted explain for some reason
  if (_explain) {
    try {
      return _explain(bindQql(trimmed, params));
    } catch {
      return _explain(trimmed);
    }
  }
  throw new Error("explain() is not available from the WASM module");
}

export function compileQql(source: string, params?: QqlParams | null): CompiledRoute {
  if (!_compile) {
    const result = analyzeQql(source, params);
    if (result.route) return result.route;
    if (result.routes.length === 1) return result.routes[0];
    if (result.error) throw new Error(`${result.error.code}: ${result.error.message}`);
    throw new Error("compile() is not available from the WASM module");
  }
  return _compile(source, params ?? undefined);
}

export function tokenizeQql(source: string): unknown[] {
  if (!_tokenize) {
    return analyzeQql(source).tokens;
  }
  return _tokenize(source);
}

export function parseQql(source: string): unknown[] {
  if (!_parse) {
    const result = analyzeQql(source);
    return result.ast ?? [];
  }
  return _parse(source);
}

export function isValidQql(source: string): boolean {
  if (!_isValid) {
    return analyzeQql(source).valid;
  }
  return _isValid(source);
}

/**
 * Format a QQL string into canonical form.
 *
 * Throws on parse errors; returns the canonical rendering (statements joined
 * by newlines, each terminated by `;`). The output has no trailing newline,
 * matching the CLI `qql fmt` contract.
 */
export function formatQql(source: string): string {
  if (!_format) {
    throw new Error("format() is not available from the WASM module");
  }
  return _format(source);
}
