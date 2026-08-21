/* tslint:disable */
/* eslint-disable */

export interface ExecuteOptions {
    onError?: "stop" | "continue";
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

export interface Token {
    kind: string;
    text: string;
    pos: number;
    end: number;
    len: number;
}

export interface CompiledRoute {
    stmt_type: string;
    method: string;
    path: string;
    payload: unknown | null;
}

export interface AnalysisError {
    code: string;
    message: string;
    start: number | null;
    end: number | null;
}

export interface AnalysisResult {
    valid: boolean;
    statements_count: number;
    tokens: Token[];
    ast: unknown[] | null;
    route: CompiledRoute | null;
    routes: CompiledRoute[];
    explain: string | null;
    error: AnalysisError | null;
}



export class Client {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Parse and compile one statement without executing it.
     */
    compile(query: string): CompiledRoute;
    /**
     * Parse, compile, embed if needed, and POST to Qdrant's REST API.
     *
     * Accepts a string (single statement or semicolon-delimited script) or
     * a `string[]`. Always returns a stable `ExecutionReport` object:
     * `{ "ok": bool, "results": [...], "succeeded": N, "failed": M }`.
     */
    execute(query: string | string[], options?: ExecuteOptions): Promise<ExecutionReport>;
    /**
     * Execute a pre-parsed Stmt object.  Injects embeddings for UPSERT
     * if an embedder is configured.
     */
    executeStmt(stmt: Stmt): Promise<ExecutionReport>;
    /**
     * Parse and explain the query — no server needed.
     */
    explain(query: string): string;
    /**
     * Check whether any embedder is configured.
     */
    hasEmbedder(): boolean;
    constructor(url?: string | null, api_key?: string | null);
    /**
     * Set a JS embedder: `async (texts: string[]) => number[][]`.
     * Called with the full batch — do not loop one-by-one inside the callback
     * if your model supports batching (Transformers.js pipeline, etc.).
     */
    setEmbedder(fn_: (texts: string[]) => Promise<number[][]> | number[][]): void;
    /**
     * OpenAI-compatible HTTP embedder. **No default URL** — pass the full
     * embeddings endpoint you intend to use, e.g.:
     * - `https://api.openai.com/v1/embeddings`
     * - `http://localhost:11434/v1/embeddings` (Ollama)
     * - any provider that accepts `{"model","input":[...]}` and returns
     *   `{"data":[{"embedding":[...],"index":0},...]}`.
     *
     * Always sends the whole text batch in one request (`input` as array).
     */
    setHttpEmbedder(endpoint: string, model: string, dimension: number, api_key?: string | null): void;
    /**
     * Alias for [`set_http_embedder`] — same OpenAI-compatible protocol.
     */
    setRemoteEmbedder(endpoint: string, model: string, dimension: number, api_key?: string | null): void;
    /**
     * Set Qdrant 1.19 read affinity. Pins reads to a stable replica via the
     * `X-Qdrant-Route-Affinity` header. Pass `null`/`""` to clear.
     */
    setRouteAffinity(affinity?: string | null): void;
    /**
     * Current read-affinity key, or `null` when unset.
     */
    readonly routeAffinity: string | undefined;
}

export class Stmt {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Compile this Stmt AST directly into a Qdrant REST route object.
     */
    compileRoute(): CompiledRoute;
    /**
     * Compile this Stmt AST into a JS-owned Uint8Array byte buffer.
     */
    compileRouteBytes(): Uint8Array;
    /**
     * Inject a WHERE filter into this statement's AST (mutates in place).
     */
    injectFilter(field: string, op: string, value: any): void;
    /**
     * Parse a QQL string into a Stmt object for programmatic manipulation.
     */
    constructor(input: string);
    /**
     * Serialise the AST to a JSON string.
     */
    toJSON(): string;
    /**
     * Serialise the AST to a JS object.
     */
    toObject(): any;
    /**
     * QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL.
     */
    get shardKey(): string | undefined;
    set shardKey(value: string | null | undefined);
}

export function analyze(input: string): AnalysisResult;

/**
 * Compile one QQL statement into a JavaScript route object.
 */
export function compile(query: string): CompiledRoute;

/**
 * Compiles QQL query into a safe, JS-owned Uint8Array byte buffer.
 */
export function compileBytes(query: string): Uint8Array;

export function explain(query: string): string;

export function explainBytes(query: string): Uint8Array;

/**
 * Format a QQL string into canonical form.
 */
export function formatQuery(input: string): string;

export function inject_filter(query: string, field: string, op: string, value: any): any;

export function isValid(input: string): boolean;

export function parse(input: string): unknown[];

export function tokenize(input: string): any[];
