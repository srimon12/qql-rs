/** Shared types matching the qql-wasm AnalysisResult surface. */

export interface AnalysisError {
  code: string;
  message: string;
  start: number | null;
  end: number | null;
}

export interface QqlToken {
  kind: string;
  text: string;
  pos: number;
  end: number;
  len: number;
}

export interface CompiledRoute {
  stmt_type: string;
  method: string | null;
  path: string | null;
  payload: unknown | null;
}

export interface WasmAnalyzeResult {
  valid: boolean;
  statements_count: number;
  tokens: QqlToken[];
  ast: unknown[] | null;
  route: CompiledRoute | null;
  routes: CompiledRoute[];
  explain: string | null;
  error: AnalysisError | null;
}

/** A top-level statement span derived from tokens / analysis. */
export interface StatementSpan {
  index: number;
  /** 0-based byte offset start */
  start: number;
  /** 0-based byte offset end (exclusive) */
  end: number;
  /** Leading statement keyword (QUERY, COUNT, CREATE, …) */
  kind: string;
  /** Short label for outline / codelens */
  label: string;
  source: string;
  route?: CompiledRoute;
}
