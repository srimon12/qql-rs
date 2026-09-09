export class Stmt {
  /** Parse a QQL string into a statement handle (mirrors `qql-wasm`). */
  constructor(input: string);
  injectFilter(field: string, op: string, value: unknown): void;
  toObject(): unknown;
  toJson(): string;
  toJSON(): string;
  /** Canonical, re-parseable QQL (mirrors Python `str(stmt)`). */
  toString(): string;
  /** Human-readable preview; long vectors are truncated (mirrors Python `repr(stmt)`). */
  toReadableString(): string;
  /** Bind `:name` (object) / `?` (array) params into this statement; returns a new bound Stmt.
   * Vector params accept plain arrays or Float32Array / Float64Array (one memcpy). */
  bind(params?: Record<string, unknown> | unknown[]): Stmt;
  compileRoute(params?: Record<string, unknown> | unknown[]): CompiledRoute;
  /** QQL `SHARD` routing key (request-level). Prefer the clause in QQL.
   * Reads back `string` (keyword) or `bigint` (numeric); set with
   * `string | number | bigint | null` (numbers must be exact integers). */
  shardKey?: string | number | bigint | null;
}

export class ScoredPoint {
  id: string | number;
  score: number;
  payload: Record<string, unknown> | null;
  text: string | null;
  collection: string | null;
  get(key: string, defaultValue?: unknown): unknown;
  [key: string]: unknown;
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

export interface ClientOptions {
  url?: string;
  apiKey?: string;
  api_key?: string;
  useGrpc?: boolean;
  use_grpc?: boolean;
  /** Qdrant 1.19+ read affinity: pins reads to a stable replica via
   * `X-Qdrant-Route-Affinity` (REST header) / `x-qdrant-route-affinity`
   * (gRPC metadata). Empty string is treated as unset. */
  routeAffinity?: string;
  route_affinity?: string;
  embedder?: HttpEmbedder | HttpEmbedderOptions;
}

export interface HttpEmbedderOptions {
  /** OpenAI-compatible dense embedding endpoint */
  endpoint: string;
  apiKey?: string;
  api_key?: string;
  model: string;
  dimension: number;
  /** Multi/ColBERT embedding endpoint (requires `endpoint`) */
  multiEndpoint?: string;
  multi_endpoint?: string;
  multiApiKey?: string;
  multi_api_key?: string;
  multiModel?: string;
  multi_model?: string;
  multiDimension?: number;
  multi_dimension?: number;
  /** Image/CLIP embedding endpoint (requires `endpoint`) */
  imageEndpoint?: string;
  image_endpoint?: string;
  imageApiKey?: string;
  image_api_key?: string;
  imageModel?: string;
  image_model?: string;
  imageDimension?: number;
  image_dimension?: number;
  /** Cross-encoder reranking endpoint (requires `endpoint`) */
  rerankEndpoint?: string;
  rerank_endpoint?: string;
  rerankApiKey?: string;
  rerank_api_key?: string;
  rerankModel?: string;
  rerank_model?: string;
}

export class HttpEmbedder {
  constructor(options: HttpEmbedderOptions);
}

export class Client {
  constructor(options?: ClientOptions);
  execute(
    query: string | Stmt | string[] | Stmt[],
    options?: ExecuteOptions,
  ): Promise<ExecutionReport>;
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
  explain(query: string): string;
  explainStmt(stmt: Stmt): string;
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
  /** Qdrant 1.19+ read affinity key set at construction; `null` when unset. */
  readonly routeAffinity: string | null;
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
export function compileQuery(
  query: string,
  params?: Record<string, unknown> | unknown[],
): CompiledRoute;
export function explain(query: string): string;
export function explainStmt(stmt: Stmt): string;
/** Substitute `:name` (object) or `?` (array) placeholders into a query string
 * or Stmt. Stmt inputs return a bound Stmt (a string when `truncateVectors`). */
export function bind(
  query: string | Stmt,
  params?: Record<string, unknown> | unknown[],
  options?: { truncateVectors?: boolean },
): string | Stmt;
export function execute(
  query: string | Stmt | string[] | Stmt[],
  options?: ExecuteOptions & ClientOptions,
): Promise<ExecutionReport>;
export function executeHits(
  query: string | Stmt | string[] | Stmt[],
  options?: ExecuteOptions & ClientOptions,
): Promise<ScoredPoint[]>;
export function executeStmt(
  stmt: Stmt,
  options?: ClientOptions & ExecuteOptions,
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
