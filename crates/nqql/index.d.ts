export class Stmt {
  injectFilter(field: string, op: string, value: unknown): void;
  toObject(): unknown;
  toJson(): string;
  toJSON(): string;
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
  endpoint: string;
  apiKey?: string;
  api_key?: string;
  model: string;
  dimension: number;
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
