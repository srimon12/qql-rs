export class Stmt {
  /** Parse a QQL string into a statement handle (mirrors `qql-wasm`). */
  constructor(input: string);
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

export interface ClientOptions {
  url?: string;
  apiKey?: string;
  api_key?: string;
  useGrpc?: boolean;
  use_grpc?: boolean;
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
  explain(query: string): string;
  explainStmt(stmt: Stmt): string;
  compile(query: string): CompiledRoute;
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
export function execute(
  query: string | Stmt | string[] | Stmt[],
  options?: ExecuteOptions & ClientOptions,
): Promise<ExecutionReport>;
export function executeStmt(
  stmt: Stmt,
  options?: ClientOptions,
): Promise<ExecutionReport>;
export const version: string;
export const __version__: string;
