export class Stmt {
  injectFilter(field: string, op: string, value: unknown): void;
  toObject(): unknown;
  toJson(): string;
  toJSON(): string;
  /** QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL. */
  shardKey?: string | null;
}

export interface ExecResponse {
  ok: boolean;
  operation: string;
  message: string;
  data: unknown | null;
}

export interface ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
}

export interface ExecuteOptions {
  onError?: "stop" | "continue";
}

export interface CompiledRoute {
  stmt_type: string;
  method: string;
  path: string;
  payload: unknown | null;
}

export interface EmbeddingModelInfo {
  /** Enum-style name, e.g. "BGESmallENV15" */
  name: string;
  /** HuggingFace model code, e.g. "Xenova/bge-small-en-v1.5" */
  modelCode: string;
  /** Dense vector dimension */
  dim: number;
  description: string;
}

export interface LocalExecutorOptions {
  /** Store payloads on disk (default true) */
  onDiskPayload?: boolean;
  /**
   * Local ONNX model. Accepts enum names (`BGESmallENV15`), HF codes
   * (`Xenova/bge-small-en-v1.5`), or short aliases (`bge-small-en-v1.5`).
   * Default: BGESmallENV15 (384-d).
   */
  model?: string;
  /** Override model cache directory */
  cacheDir?: string;
  /** Show HuggingFace download progress (default false) */
  showDownloadProgress?: boolean;
}

export interface StandaloneOptions {
  /** Path to the local Qdrant-compatible data directory (default "./qdrant_data") */
  dataDir?: string;
  /** Store payloads on disk (default true) */
  onDiskPayload?: boolean;
  /** Local ONNX model for standalone execute() / executeStmt() */
  model?: string;
  /** Override model cache directory */
  cacheDir?: string;
  /** Show HuggingFace download progress */
  showDownloadProgress?: boolean;
  /** Override: HTTP embedding endpoint URL */
  embedUrl?: string;
  /** Override: Bearer token for HTTP embedding */
  embedKey?: string;
  /** Override: model name for HTTP embedding */
  embedModel?: string;
  /** Override: output dimension for HTTP embedding */
  embedDim?: number;
  /** onError behaviour */
  onError?: "stop" | "continue";
}

export class Client {
  /**
   * Execute a QQL query string, Stmt, or array of either.
   * Multi-statement strings (semicolons) and arrays are auto-batched.
   */
  execute(
    query: string | Stmt | string[] | Stmt[],
    options?: ExecuteOptions,
  ): Promise<ExecutionReport>;

  /** Explain a QQL query string — returns a human-readable plan. */
  explain(query: string): string;

  /** Explain a pre-parsed Stmt. */
  explainStmt(stmt: Stmt): string;

  /** Compile a QQL query to its transport route (non-executing). */
  compile(query: string): CompiledRoute;

  /** Flush and release local edge storage. Idempotent. */
  close(): Promise<void>;
}

/** Parse one statement or a semicolon-delimited script into a stable list. */
export function parse(query: string): Stmt[];

/** Parse to raw JSON string — bypasses V8 object allocation. */
export function parseJson(query: string): string;

export function isValid(query: string): boolean;

export function injectFilter(
  query: string,
  field: string,
  op: string,
  value: unknown,
): unknown;

export function tokenize(
  query: string,
): Array<{ kind: string; text: string; pos: number }>;

export function compileQuery(query: string): CompiledRoute;

export function explain(query: string): string;

export function explainStmt(stmt: Stmt): string;

/**
 * Create a fully-local edge executor backed by fastembed-rs.
 * Models download from HuggingFace on first use and cache locally.
 * No network calls for inference — embedding runs on-device via ONNX.
 *
 * @param dataDir local data directory
 * @param options boolean is legacy `onDiskPayload`; object is preferred
 */
export function localExecutor(
  dataDir: string,
  options?: boolean | LocalExecutorOptions,
): Client;

/** List dense ONNX models available for `localExecutor({ model })`. */
export function listEmbeddingModels(): EmbeddingModelInfo[];

/**
 * Create an edge executor that calls an external OpenAI-compatible embedding
 * endpoint. Vector storage/search is still fully local.
 */
export function httpExecutor(
  dataDir: string,
  url: string,
  embedKey: string,
  embedModel: string,
  embedDim: number,
  onDiskPayload?: boolean,
): Client;

/**
 * One-shot execute with a temporary edge client.
 * Loads the ONNX model every call — prefer a long-lived Client.
 */
export function execute(
  query: string | Stmt | string[] | Stmt[],
  options?: ExecuteOptions & StandaloneOptions,
): Promise<ExecutionReport>;

/**
 * One-shot execute of a pre-parsed Stmt with a temporary edge client.
 * Loads the ONNX model every call — prefer a long-lived Client.
 */
export function executeStmt(
  stmt: Stmt,
  options?: StandaloneOptions,
): Promise<ExecutionReport>;

export const version: string;
export const __version__: string;
