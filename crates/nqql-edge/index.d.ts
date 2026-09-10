export class Stmt {
  constructor(input: string);
  injectFilter(field: string, op: string, value: unknown): void;
  toObject(): unknown;
  toJson(): string;
  toJSON(): string;
  /** Bind `:name` (object) / `?` (array) params into this statement; returns a new bound Stmt.
   * Vector params accept plain arrays or Float32Array / Float64Array (one memcpy). */
  bind(params?: Record<string, unknown> | unknown[]): Stmt;
  /** Canonical, re-parseable QQL (mirrors Python `str(stmt)`). */
  toString(): string;
  /** Human-readable preview; long vectors are truncated (mirrors Python `repr(stmt)`). */
  toReadableString(): string;
  compileRoute(params?: Record<string, unknown> | unknown[]): CompiledRoute;
  /** QQL `SHARD` routing key (request-level). Prefer the clause in QQL.
   * Reads back `string` (keyword) or `bigint` (numeric); set with
   * `string | number | bigint | null` (numbers must be exact integers). */
  shardKey?: string | number | bigint | null;
}

export class ScoredPoint {
  id: number | string;
  score: number;
  payload: Record<string, unknown> | null;
  text: string | null;
  collection: string | null;
  [key: string]: unknown;
  get(key: string, defaultValue?: unknown): unknown;
}

export interface ExecResponse {
  ok: boolean;
  operation: string;
  message: string;
  data: unknown | null;
  /** Server telemetry when the backend reported it; absent otherwise. */
  telemetry?: ServerTelemetry | null;
}

export interface HardwareUsage {
  cpu: number;
  payload_io_read: number;
  payload_io_write: number;
  payload_index_io_read: number;
  payload_index_io_write: number;
  vector_io_read: number;
  vector_io_write: number;
}

export interface ModelUsage {
  tokens: number;
}

export interface InferenceUsage {
  models: Record<string, ModelUsage>;
}

export interface ServerUsage {
  hardware?: HardwareUsage | null;
  inference?: InferenceUsage | null;
}

export interface ServerTelemetry {
  time_s?: number | null;
  usage?: ServerUsage | null;
}

export interface PhaseTimings {
  parse_ms: number;
  prepare_plan_ms: number;
  dispatch_ms: number;
  decode_ms: number;
  total_ms: number;
}

/** Structured `EXPLAIN ANALYZE` report (see `Client.explainAnalyze`). */
export interface AnalyzeReport {
  ok: boolean;
  plan: string;
  phases: PhaseTimings;
  server_time_s?: number | null;
  usage?: ServerUsage | null;
  results: ExecResponse[];
}

export class ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
  /** Aggregated server telemetry when the backend reported it; absent otherwise. */
  telemetry?: ServerTelemetry | null;
  [key: string]: unknown;
  hits(stmt?: number): ScoredPoint[];
  points(stmt?: number): ScoredPoint[];
  ids(stmt?: number): Array<string | number>;
  facet(stmt?: number): Array<{ value: unknown; count: number }>;
  count(stmt?: number): number;
  groups(stmt?: number): Array<{ id: unknown; hits: Array<Record<string, unknown>> }>;
}

export interface ExecuteOptions {
  onError?: "stop" | "continue";
  params?: Record<string, unknown> | unknown[];
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
  /** Model supports multivector (ColBERT-style) embeddings */
  multi: boolean;
  /** Model supports CLIP vision embeddings */
  image: boolean;
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
  /** Offline sparse model (SPLADE or BGE-M3 SparseTextEmbedding), e.g. "splade". None → local wire-compatible BM25 (Qdrant qdrant/bm25-identical token IDs). */
  sparseModel?: string;
  /** Offline multivector model (BGE-M3 ColBERT), e.g. "bge-m3". */
  multiModel?: string;
  /** Offline CLIP vision model, e.g. "clip-vision" / "ClipVitB32". */
  imageModel?: string;
  /** Offline cross-encoder, e.g. "bge-reranker-base". */
  rerankerModel?: string;
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
  /** Offline sparse model (SPLADE or BGE-M3 SparseTextEmbedding), e.g. "splade". None → local wire-compatible BM25 (Qdrant qdrant/bm25-identical token IDs). */
  sparseModel?: string;
  /** Offline multivector model (BGE-M3 ColBERT), e.g. "bge-m3". */
  multiModel?: string;
  /** Offline CLIP vision model, e.g. "clip-vision" / "ClipVitB32". */
  imageModel?: string;
  /** Offline cross-encoder, e.g. "bge-reranker-base". */
  rerankerModel?: string;
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
  /** Parameter bindings */
  params?: Record<string, unknown> | unknown[];
}

/**
 * Edge executor client. Not directly constructible — the constructor always
 * throws. Create instances via {@link localExecutor} or {@link httpExecutor}.
 */
export class Client {
  /**
   * Execute a QQL query string, Stmt, or array of either.
   * Multi-statement strings (semicolons) and arrays are auto-batched.
   */
  execute(
    query: string | Stmt | string[] | Stmt[],
    options?: ExecuteOptions,
  ): Promise<ExecutionReport>;

  /** Execute a query and return typed ScoredPoint hits directly. */
  executeHits(
    query: string | Stmt | string[] | Stmt[],
    options?: ExecuteOptions,
  ): Promise<ScoredPoint[]>;
  /**
   * Analyze a single query string or Stmt: static plan plus measured
   * execution (per-phase client timings, server time, hardware/inference
   * usage). Batches fail closed — analyze each entry separately.
   */
  explainAnalyze(
    query: string | Stmt,
    options?: ExecuteOptions,
  ): Promise<AnalyzeReport>;
  /**
   * Bulk ingest point objects (`{id, vector, …payload}`) in `batchSize`
   * chunks (default 100). One `:rows` template is prepared once — no
   * re-parse, no per-batch schema fetch. Vectors take plain arrays, packed
   * `Float32Array` / `Float64Array`, integer typed arrays for sparse
   * `indices`, or the flat `{data, dim}` multivector form.
   */
  upsertMany(
    collection: string,
    rows: Record<string, unknown>[],
    options?: ExecuteOptions & { batchSize?: number },
  ): Promise<ExecutionReport>;

  /** Explain a QQL query string — returns a human-readable plan. */
  explain(query: string): string;

  /** Explain a pre-parsed Stmt. */
  explainStmt(stmt: Stmt): string;

  /** Compile a QQL query to its transport route (non-executing). */
  compile(query: string, params?: Record<string, unknown> | unknown[]): CompiledRoute;

  /** Lazily page through a collection with SCROLL, yielding one ScoredPoint per point. */
  scrollCursor(
    collection: string,
    options?: ScrollCursorOptions,
  ): AsyncGenerator<ScoredPoint>;

  /** WHATWG stream over `scrollCursor` (pull-driven; honors backpressure). */
  scrollStream(
    collection: string,
    options?: ScrollCursorOptions,
  ): ReadableStream<ScoredPoint>;

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
): Array<{ kind: string; text: string; pos: number; end: number; len: number }>;

export function compileQuery(query: string, params?: Record<string, unknown> | unknown[]): CompiledRoute;

export function explain(query: string): string;

export function explainStmt(stmt: Stmt): string;

/** Substitute `:name` (object) or `?` (array) placeholders into a query string
 * or Stmt. Stmt inputs return a bound Stmt (a string when `truncateVectors`). */
export function bind(
  query: string | Stmt,
  params?: Record<string, unknown> | unknown[],
  options?: { truncateVectors?: boolean },
): string | Stmt;

/** One-shot execute returning typed ScoredPoint hits directly. */
export function executeHits(
  query: string | Stmt | string[] | Stmt[],
  options?: ExecuteOptions & StandaloneOptions,
): Promise<ScoredPoint[]>;

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

export interface ScrollCursorOptions {
  /** Points per SCROLL page; a positive integer (default 100). */
  batchSize?: number;
  /** Raw QQL filter fragment appended as `WHERE …` (default none). */
  where?: string;
  /** Optional parameters to bind into the `where` filter fragment. */
  params?: Record<string, unknown>;
  /** Payloads are included by default; `false` strips `payload`
   * client-side before yielding (SCROLL has no server-side payload
   * exclusion in the grammar). */
  withPayload?: boolean;
  /** Append `WITH VECTOR` so yielded points carry vectors (default false;
   * SCROLL omits vectors unless asked). */
  withVector?: boolean;
  /** Optional custom shard key partition routing (keyword or number). */
  shardKey?: string | number | bigint;
}
/** Lazily page through a collection with SCROLL, yielding one ScoredPoint
 * per point. At most one page is ever buffered. Works with any client
 * exposing `execute(sql, { params })`. */
export function scrollCursor(
  client: Client,
  collection: string,
  options?: ScrollCursorOptions,
): AsyncGenerator<ScoredPoint>;
/** WHATWG stream over `scrollCursor` (pull-driven; honors backpressure). */
export function scrollStream(
  client: Client,
  collection: string,
  options?: ScrollCursorOptions,
): ReadableStream<ScoredPoint>;

export const version: string;
export const __version__: string;
