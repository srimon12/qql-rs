/**
 * WASM parser bridge — imports the bundled qql-wasm Node.js package
 * and exposes analyze() for diagnostics.
 *
 * The qql-wasm Node.js target synchronously loads the .wasm binary
 * at module initialization time. No async init() call is needed.
 */

import type { WasmAnalyzeResult } from "./diagnostics";

// Re-export for extension.ts
export type { WasmAnalyzeResult };

let _analyze: ((input: string) => WasmAnalyzeResult) | null = null;

export function initWasm(): void {
  if (_analyze) return;

  // The Node.js wasm-pack target synchronously reads the .wasm binary
  // and exports analyze() as a direct module property.
  try {
    const qqlWasm = require("../wasm/qql_wasm.js");
    if (typeof qqlWasm.analyze !== "function") {
      throw new Error("qql-wasm module loaded but analyze() is not a function");
    }
    _analyze = qqlWasm.analyze as (input: string) => WasmAnalyzeResult;
  } catch (err) {
    throw new Error(
      `Failed to load QQL WASM parser: ${err instanceof Error ? err.message : String(err)}`
    );
  }
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

export function analyzeQql(source: string): WasmAnalyzeResult {
  if (!_analyze) {
    throw new Error("WASM parser not initialized — call initWasm() first");
  }
  if (!source.trim()) {
    return EMPTY;
  }
  return _analyze(source);
}
