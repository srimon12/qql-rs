declare module "qql-wasm" {
  export class Client {
    free(): void
    [Symbol.dispose](): void
    compile(query: string): string
    execute(query: unknown): Promise<string>
    executeStmt(stmt: Stmt): Promise<string>
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
    compileRoute(): string
    get shardKey(): string | undefined
    set shardKey(value: string | null | undefined)
  }

  export function analyze(input: string): string
  export function compile(query: string): string
  export function explain(query: string): string
  export function inject_filter(
    query: string,
    field: string,
    op: string,
    value: unknown
  ): unknown
  export function isValid(input: string): boolean
  export function parse(input: string): unknown
  export function parse_all(input: string): unknown
  export function parse_batch(queries: string[]): unknown
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
