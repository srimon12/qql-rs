export class Stmt {
  /** Parse a QQL string into a statement handle (mirrors `qql-wasm`). */
  constructor(input: string);
  injectFilter(field: string, op: string, value: unknown): void;
  toObject(): unknown;
  toJson(): string;
  toJSON(): string;
  toString(): string;
  bind(
    params: Record<string, unknown> | unknown[],
    options?: { truncateVectors?: boolean },
  ): Stmt;
  compileRoute(params?: Record<string, unknown> | unknown[]): CompiledRoute;
  /** QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL. */
  shardKey?: string | null;
}

export class ScoredPoint {
  id: string | number;
  score: number;
  payload: Record<string, unknown>;
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
}

export class ExecutionReport {
  ok: boolean;
  results: ExecResponse[];
  succeeded: number;
  failed: number;
  hits(stmt?: number): ScoredPoint[];
  points(stmt?: number): unknown[];
  facet(stmt?: number): Array<{ value: unknown; count: number }>;
  count(stmt?: number): number;
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
  explain(query: string): string;
  explainStmt(stmt: Stmt): string;
  compile(query: string, params?: Record<string, unknown> | unknown[]): CompiledRoute;
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
/** Substitute `:name` (object) or `?` (array) placeholders into a query string. */
export function bind(
  query: string,
  params: Record<string, unknown> | unknown[],
  options?: { truncateVectors?: boolean },
): string;
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
  options?: ClientOptions,
): Promise<ExecutionReport>;
export const version: string;
export const __version__: string;
