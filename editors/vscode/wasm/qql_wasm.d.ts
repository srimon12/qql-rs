/* tslint:disable */
/* eslint-disable */

export interface ExecuteOptions {
    onError?: "stop" | "continue";
    params?: Record<string, unknown> | unknown[];
}

export interface ServerTelemetry {
    time_s?: number | null;
    usage?: ServerUsage | null;
}

export interface ServerUsage {
    hardware?: HardwareUsage | null;
    inference?: InferenceUsage | null;
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

export interface InferenceUsage {
    models: Record<string, { tokens: number }>;
}

export interface PhaseTimings {
    parse_ms: number;
    prepare_plan_ms: number;
    dispatch_ms: number;
    decode_ms: number;
    total_ms: number;
}

export interface ExecResponse {
    ok: boolean;
    operation: string;
    message: string;
    data: unknown | null;
    telemetry?: ServerTelemetry | null;
}

export interface ExecutionReport {
    ok: boolean;
    results: ExecResponse[];
    succeeded: number;
    failed: number;
    telemetry?: ServerTelemetry | null;
    hits(stmt?: number): Array<Record<string, unknown>>;
    points(stmt?: number): Array<Record<string, unknown>>;
    ids(stmt?: number): Array<string | number>;
    facet(stmt?: number): Array<{ value: unknown; count: number }>;
    count(stmt?: number): number;
    groups(stmt?: number): Array<{ id: unknown; hits: Array<Record<string, unknown>> }>;
}

export interface AnalyzeReport {
    ok: boolean;
    plan: string;
    phases: PhaseTimings;
    server_time_s: number | null;
    usage: ServerUsage | null;
    results: ExecResponse[];
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
    /** First error, kept for older IDE clients. Prefer `errors`. */
    error: AnalysisError | null;
    /** Every recoverable diagnostic from panic-mode parse + plan. */
    errors?: AnalysisError[];
}



export class Client {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Parse and compile one statement without executing it. Alias for `compile`.
     */
    compileQuery(query: string, params?: any | null): CompiledRoute;
    /**
     * Parse and compile one statement without executing it. Optional
     * `params` bind before parsing (same shape as the module-level `bind`).
     */
    compile(query: string, params?: any | null): CompiledRoute;
    /**
     * Execute a pre-parsed Stmt object. Injects embeddings for UPSERT
     * if an embedder is configured. Optionally binds parameters.
     */
    executeStmt(stmt: Stmt, options?: ExecuteOptions): Promise<ExecutionReport>;
    /**
     * Parse, compile, embed if needed, and POST to Qdrant's REST API.
     *
     * Accepts a string, a Stmt, or an array of either. Always returns a stable
     * `ExecutionReport` object:
     * `{ "ok": bool, "results": [...], "succeeded": N, "failed": M }`.
     */
    execute(query: string | Stmt | (string | Stmt)[], options?: ExecuteOptions): Promise<ExecutionReport>;
    /**
     * Analyze a single query string or `Stmt`: static plan plus measured
     * execution (per-phase client timings, server time, hardware and
     * inference usage). Returns an `AnalyzeReport` object:
     * `{ ok, plan, phases, server_time_s, usage, results }`.
     * Batches fail closed. `options.params` binds before analysis.
     */
    explainAnalyze(query: string | Stmt, options?: ExecuteOptions): Promise<AnalyzeReport>;
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
     * Set client-side BM25 document parameters for the built-in local sparse
     * encoder (`k1`, `b`, `avg_len`). **Write-path only**: shapes how
     * documents upserted after the call are encoded; query weights stay unit
     * and server-side inference is untouched. Invalid values throw
     * (`QQL-VALIDATION-CONFIG`): `k1 > 0`, `b` in `[0, 1]`, `avg_len > 0`,
     * all finite.
     */
    setBm25Params(k1: number, b: number, avg_len: number): void;
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
     * OpenAI-compatible image/CLIP vision endpoint (dense vectors).
     * Browser calls need a CORS-enabled endpoint.
     */
    setHttpImageEmbedder(endpoint: string, model: string, dimension: number, api_key?: string | null): void;
    /**
     * OpenAI-compatible multi/ColBERT endpoint (nested `[[...]]` bags).
     * Browser calls need a CORS-enabled endpoint.
     */
    setHttpMultiEmbedder(endpoint: string, model: string, dimension: number, api_key?: string | null): void;
    /**
     * Cohere-compatible cross-encoder rerank endpoint.
     * Browser calls need a CORS-enabled endpoint.
     */
    setHttpReranker(endpoint: string, model: string, api_key?: string | null): void;
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
     * Bulk ingest: `rows` is an array of point objects
     * (`{id, vector, …payload}`) spliced through the `:rows` point-splice
     * path in `batchSize` chunks (default 100). Row vectors accept plain
     * arrays, `Float32Array` / `Float64Array` (packed, one copy), integer
     * typed arrays (sparse `indices`), and the flat `{data, dim}`
     * multivector form — the same inputs as `bind`.
     */
    upsertMany(collection: string, rows: any, options?: any | null): Promise<ExecutionReport>;
    /**
     * Current read-affinity key, or `null` when unset.
     */
    readonly routeAffinity: string | undefined;
}

export class Stmt {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Bind parameters into this statement and return a new bound Stmt.
     */
    bind(params?: any | null): Stmt;
    /**
     * Compile this Stmt AST into a JS-owned Uint8Array byte buffer.
     */
    compileRouteBytes(): Uint8Array;
    /**
     * Compile this Stmt AST directly into a Qdrant REST route object.
     * Optionally accepts `params` to bind before compiling.
     */
    compileRoute(params?: any | null): CompiledRoute;
    /**
     * Explain this statement's execution plan (mirrors the free `explain`).
     */
    explain(): string;
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
     * Format statement as a human-readable preview (mirrors Python `repr(stmt)`):
     * long vector literals are truncated, so the output may not re-parse.
     */
    toReadableString(): string;
    /**
     * Format statement as canonical, re-parseable QQL (mirrors Python `str(stmt)`).
     */
    toString(): string;
    /**
     * Whether parameters have already been bound into this statement.
     */
    readonly bound: boolean;
    /**
     * QQL `SHARD` routing key (request-level). Prefer the clause in QQL.
     *
     * Reads back a string for keyword keys, a `BigInt` for numeric keys, and
     * `null` when unset (placeholders also read as `null` — bind first).
     * The setter accepts string, number, `BigInt`, or null: numbers must be
     * exact non-negative integers (larger keys need `BigInt`).
     */
    shardKey: any;
}

export function analyze(input: string): AnalysisResult;

/**
 * Substitute `:name` (object) or `?` (array) placeholders into a query string.
 * Without `params`, the query is returned unchanged (mirrors pyqql `bind`).
 */
export function bind(query: string, params?: Record<string, unknown> | unknown[], options?: { truncateVectors?: boolean }): string;

/**
 * Compile one QQL statement into a JavaScript route object. Optional
 * `params` (object for `:name`, array for `?`) bind before parsing —
 * parity with `Client.compile(query, params)` on the Python and Node SDKs.
 */
export function compile(query: string, params?: any | null): CompiledRoute;

/**
 * Compiles QQL query into a safe, JS-owned Uint8Array byte buffer.
 */
export function compileBytes(query: string): Uint8Array;

/**
 * Compile one QQL statement into a JavaScript route object. Alias for `compile`.
 */
export function compileQuery(query: string, params?: any | null): CompiledRoute;

export function explain(query: string): string;

export function explainBytes(query: string): Uint8Array;

/**
 * Format a QQL string into canonical form.
 */
export function formatQuery(input: string): string;

export function inject_filter(query: string, field: string, op: string, value: any): any;

export function isValid(input: string): boolean;

export function parse(input: string): unknown[];

/**
 * Parse to a raw JSON string of the AST array — no JS object allocation,
 * mirroring `parseJson` on the Node SDK.
 */
export function parseJson(input: string): string;

export function tokenize(input: string): any[];
