declare module "qql-wasm" {
  export interface ExecuteOptions {
    onError?: "stop" | "continue"
  }

  export interface ExecResponse {
    ok: boolean
    operation: string
    message: string
    data: unknown | null
  }

  export interface ExecutionReport {
    ok: boolean
    results: ExecResponse[]
    succeeded: number
    failed: number
  }

  export interface CompiledRoute {
    stmt_type: string
    method: string
    path: string
    payload: unknown | null
  }

  export interface AnalysisResult {
    valid: boolean
    statements_count: number
    tokens: Array<{
      kind: string
      text: string
      pos: number
      end: number
      len: number
    }>
    ast: unknown[] | null
    route: CompiledRoute | null
    routes: CompiledRoute[]
    explain: string | null
    error: {
      code: string
      message: string
      start: number | null
      end: number | null
    } | null
  }

  export class Client {
    free(): void
    [Symbol.dispose](): void
    compile(query: string): CompiledRoute
    execute(
      query: string | string[],
      options?: ExecuteOptions
    ): Promise<ExecutionReport>
    executeStmt(stmt: Stmt): Promise<ExecutionReport>
    explain(query: string): string
    hasEmbedder(): boolean
    constructor(url?: string | null, api_key?: string | null)
    setEmbedder(fn_: (texts: string[]) => Promise<number[][]> | number[][]): void
    setHttpEmbedder(
      endpoint: string,
      model: string,
      dimension: number,
      api_key?: string | null
    ): void
    setRemoteEmbedder(
      endpoint: string,
      model: string,
      dimension: number,
      api_key?: string | null
    ): void
  }

  export class Stmt {
    free(): void
    [Symbol.dispose](): void
    injectFilter(field: string, op: string, value: unknown): void
    constructor(input: string)
    toJSON(): string
    toObject(): unknown
    compileRoute(): CompiledRoute
    get shardKey(): string | undefined
    set shardKey(value: string | null | undefined)
  }

  export function analyze(input: string): AnalysisResult
  export function compile(query: string): CompiledRoute
  export function compileBytes(query: string): Uint8Array
  export function explain(query: string): string
  export function inject_filter(
    query: string,
    field: string,
    op: string,
    value: unknown
  ): unknown
  export function isValid(input: string): boolean
  /** Parse a QQL source (single or semicolon-delimited) into an array of ASTs. */
  export function parse(input: string): unknown[]
  export function tokenize(input: string): unknown[]

  export default function init(
    module_or_path?:
      | RequestInfo
      | URL
      | Response
      | BufferSource
      | WebAssembly.Module
      | Promise<RequestInfo | URL | Response | BufferSource | WebAssembly.Module>
      | { module_or_path: unknown }
  ): Promise<unknown>
}
