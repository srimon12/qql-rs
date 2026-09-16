import { indentWithTab } from "@codemirror/commands";
import {
	type Diagnostic,
	forceLinting,
	linter,
	lintGutter,
} from "@codemirror/lint";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { basicSetup } from "codemirror";
import initQql, {
	analyze,
	Client,
	type CompiledRoute,
	formatQuery,
	Stmt,
} from "qql-wasm-current";
import { createBrowserEmbedder } from "./browser-embedder";
import {
	qqlCompletion,
	qqlHighlighting,
	qqlLanguage,
} from "./playground-language";
import {
	DEFAULT_POLICY,
	DEFAULT_SETTINGS,
	type ExportLanguage,
	type InspectorTab,
	type LiveCollectionTopology,
	migrateSettings,
	type PlaygroundAnalysis,
	type PlaygroundFailure,
	type PlaygroundSettings,
	type PlaygroundState,
	type PolicyValueType,
	type RuntimePolicy,
} from "./playground-types";

const SETTINGS_KEY = "qql-playground.settings.v1";
const POLICY_KEY = "qql-playground.policy.v2";
const WORKSPACE_KEY = "qql-playground.workspace.v1";
const INSPECTOR_TAB_KEY = "qql-playground.inspector-tab.v1";
const WRAP_KEY = "qql-playground.wrap.v1";
const ANALYSIS_DELAY_MS = 90;

function required<T extends Element>(selector: string): T {
	const element = document.querySelector<T>(selector);
	if (!element) throw new Error(`Playground element is missing: ${selector}`);
	return element;
}

function all<T extends Element>(selector: string): T[] {
	return [...document.querySelectorAll<T>(selector)];
}

function formatError(error: unknown): string {
	if (error instanceof Error) return error.message;
	if (typeof error === "string") return error;
	try {
		return JSON.stringify(error);
	} catch {
		return String(error);
	}
}

function humanizeFailure(
	parsed: { code: string; message: string; fields: Record<string, string> },
	host: string,
): string {
	switch (parsed.code) {
		case "QQL-MISSING-USING":
			return "This text query needs a USING clause to pick a vector.";
		case "QQL-BACKEND-COLLECTION-NOT-FOUND": {
			const name =
				parsed.fields.collection ??
				/Collection `([^`]+)`/.exec(parsed.message)?.[1] ??
				/Collection '([^']+)'/.exec(parsed.message)?.[1] ??
				"this collection";
			return `Collection '${name}' doesn't exist on this Qdrant.`;
		}
		case "QQL-BACKEND-DIMENSION-MISMATCH": {
			const dims = /expected dim: (\d+), got (\d+)/.exec(parsed.message);
			return dims
				? `Vector size doesn't match the collection (needs ${dims[1]}, got ${dims[2]}).`
				: "Vector size doesn't match the collection.";
		}
		case "QQL-TRANSPORT":
		case "QQL-TRANSPORT-CONNECT":
		case "QQL-TRANSPORT-TIMEOUT":
			return `Qdrant unreachable at ${host} — check connection`;
		default: {
			if (parsed.code.startsWith("QQL-TRANSPORT")) {
				return `Qdrant unreachable at ${host} — check connection`;
			}
			if (parsed.code.startsWith("QQL-EMBEDDING")) {
				return `The embedding endpoint failed (${parsed.code}). Check the endpoint, model, and dimension.`;
			}
			if (parsed.code.startsWith("QQL-BACKEND")) {
				const status = parsed.fields.status_code;
				return status
					? `Qdrant rejected the request (HTTP ${status}).`
					: "Qdrant rejected the request.";
			}
			return parsed.message;
		}
	}
}

interface ParsedQqlError {
	code: string;
	kind?: string;
	message: string;
	span: { start: number; end: number } | null;
	fields: Record<string, string>;
}

/** `buildError` port: WASM throws JSON-serialized QqlError strings, not Errors. */
function parseQqlError(raw: string): ParsedQqlError | null {
	let parsed: unknown;
	try {
		parsed = JSON.parse(raw);
	} catch {
		return null;
	}
	if (!parsed || typeof parsed !== "object") return null;
	const record = parsed as Record<string, unknown>;
	if (typeof record.code !== "string") return null;
	const fields: Record<string, string> = {};
	const rawFields = record.fields;
	if (Array.isArray(rawFields)) {
		for (const field of rawFields) {
			if (field && typeof field === "object") {
				const entry = field as Record<string, unknown>;
				if (typeof entry.key === "string" && entry.value != null) {
					fields[entry.key] = String(entry.value);
				}
			}
		}
	}
	const span = record.span as { start: number; end: number } | null | undefined;
	return {
		code: record.code,
		kind: typeof record.kind === "string" ? record.kind : undefined,
		message: typeof record.message === "string" ? record.message : raw,
		span:
			span && typeof span.start === "number" && typeof span.end === "number"
				? { start: span.start, end: span.end }
				: null,
		fields,
	};
}

/** Structured failure with a human sentence; keeps the parsed `.code`. */
function buildFailure(error: unknown, qdrantUrl: string): PlaygroundFailure {
	const raw = formatError(error);
	const host = connectionHost(qdrantUrl);
	const parsed = parseQqlError(raw);
	if (!parsed) {
		if (
			error instanceof TypeError ||
			/failed to fetch|networkerror|load failed|network request failed|connection refused/i.test(
				raw,
			)
		) {
			return {
				message: `Qdrant unreachable at ${host} — check connection`,
				code: "QQL-TRANSPORT",
				kind: "Transport",
				span: null,
				fields: {},
				raw,
			};
		}
		return {
			message: raw || "The query failed with an unknown error.",
			code: null,
			kind: null,
			span: null,
			fields: {},
			raw,
		};
	}
	return {
		message: humanizeFailure(parsed, host),
		code: parsed.code,
		kind: parsed.kind ?? null,
		span: parsed.span,
		fields: parsed.fields,
		raw,
	};
}

function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function pretty(value: unknown): string {
	// u64/i64 (point IDs, counts, integer payloads) cross the WASM boundary
	// as BigInt, which JSON.stringify rejects. Safe-range values render as
	// the plain digits they always were (they paste straight back into QQL);
	// larger ones keep the `n` literal suffix so snowflake IDs stay exact and
	// visibly numeric instead of rounding into a Number.
	return JSON.stringify(
		value,
		(_key, nested: unknown) =>
			typeof nested === "bigint"
				? printableBigInt(nested)
				: (nested as unknown),
		2,
	);
}

function printableBigInt(value: bigint): string {
	const digits = value.toString();
	const safe =
		value <= BigInt(Number.MAX_SAFE_INTEGER) &&
		value >= BigInt(Number.MIN_SAFE_INTEGER);
	return safe ? digits : `${digits}n`;
}

function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;");
}

/** Display-only JSON: unquote BigInt-suffixed strings so snowflakes read numeric. */
function displayJson(value: unknown): string {
	return pretty(value).replace(/"(-?\d+n)"/g, "$1");
}

const JSON_TOKEN_RE =
	/("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?n?)|\b(true|false|null)\b/g;

/** Tiny brand-palette highlighter for JSON dumps (keys/strings/numbers). */
function highlightJson(source: string): string {
	let result = "";
	let last = 0;
	source.replace(
		JSON_TOKEN_RE,
		(
			match: string,
			str: string,
			colon: string,
			num: string,
			lit: string,
			offset: number,
		) => {
			result += escapeHtml(source.slice(last, offset));
			if (str !== undefined) {
				const inner = str.slice(1, -1);
				const cls =
					colon !== undefined
						? "tok-key"
						: /^-?\d+n$/.test(inner)
							? "tok-num"
							: "tok-str";
				result += `<span class="${cls}">${escapeHtml(str)}</span>${colon === undefined ? "" : escapeHtml(colon)}`;
			} else if (num !== undefined) {
				result += `<span class="tok-num">${escapeHtml(num)}</span>`;
			} else {
				result += `<span class="tok-lit">${escapeHtml(lit)}</span>`;
			}
			last = offset + match.length;
			return match;
		},
	);
	return result + escapeHtml(source.slice(last));
}

const EXPLAIN_TOKEN_RE =
	/(QQL-[A-Z0-9-]+)|("(?:[^"\\]|\\.)*")|(-?\d+(?:\.\d+)?)/g;

/** Same palette for the human-readable plan: codes, quoted text, numbers. */
function highlightExplain(source: string): string {
	let result = "";
	let last = 0;
	source.replace(
		EXPLAIN_TOKEN_RE,
		(match: string, code: string, str: string, num: string, offset: number) => {
			result += escapeHtml(source.slice(last, offset));
			if (code !== undefined) {
				result += `<span class="tok-key">${escapeHtml(code)}</span>`;
			} else if (str !== undefined) {
				result += `<span class="tok-str">${escapeHtml(str)}</span>`;
			} else {
				result += `<span class="tok-num">${escapeHtml(num)}</span>`;
			}
			last = offset + match.length;
			return match;
		},
	);
	return result + escapeHtml(source.slice(last));
}

/** Render a JSON tab with highlighting; plain-text tabs keep raw text. */
function renderHighlighted(pre: HTMLElement, value: unknown): void {
	pre.innerHTML =
		typeof value === "string"
			? escapeHtml(value)
			: highlightJson(displayJson(value));
}

function renderExplain(pre: HTMLElement, value: unknown): void {
	pre.innerHTML =
		typeof value === "string"
			? highlightExplain(value)
			: highlightJson(displayJson(value));
}

function loadStored<T>(key: string, fallback: T): T {
	try {
		const raw = localStorage.getItem(key);
		if (!raw) return { ...fallback };
		return { ...fallback, ...(JSON.parse(raw) as Partial<T>) };
	} catch {
		return { ...fallback };
	}
}

function loadSession(key: string): string | null {
	try {
		return sessionStorage.getItem(key);
	} catch {
		return null;
	}
}

function saveSession(key: string, value: string): void {
	try {
		sessionStorage.setItem(key, value);
	} catch {
		// Private browsing may deny storage; the playground remains usable.
	}
}

function applyRuntimePolicy(statement: Stmt): void {
	statement.injectFilter(policy.field, policy.op, policyValue(policy));
	if (policy.shardKey.trim()) statement.shardKey = policy.shardKey.trim();
}

function policyValue(policy: RuntimePolicy): string | number | boolean {
	if (policy.valueType === "number") {
		const number = Number(policy.value);
		if (!Number.isFinite(number))
			throw new Error("Policy value must be a number.");
		return number;
	}
	if (policy.valueType === "boolean") {
		if (policy.value !== "true" && policy.value !== "false") {
			throw new Error("Boolean policy values must be true or false.");
		}
		return policy.value === "true";
	}
	return policy.value;
}

function byteOffsetToPosition(source: string, offset: number): number {
	const bytes = new TextEncoder().encode(source);
	const safeOffset = Math.max(0, Math.min(offset, bytes.length));
	return new TextDecoder().decode(bytes.slice(0, safeOffset)).length;
}

function quotePython(source: string): string {
	return `qql = """${source.replace(/"""/g, '\\"\\"\\"')}"""`;
}

function quoteRust(source: string): string {
	const hashes = source.includes('"#') ? "##" : "#";
	return `r${hashes}"${source}"${hashes}`;
}

function exportCode(
	language: ExportLanguage,
	source: string,
	settings: PlaygroundSettings,
	route: CompiledRoute | null,
	statementCount: number,
): string {
	const url = JSON.stringify(settings.qdrantUrl);
	const pythonKey = settings.qdrantKey
		? JSON.stringify(settings.qdrantKey)
		: "None";

	if (language === "python") {
		return `# pip install pyqql
from pyqql import Client

${quotePython(source)}
# execute() accepts a complete QQL script, including multiple statements.
client = Client(url=${url}, api_key=${pythonKey})
report = client.execute(qql)
print(report)`;
	}

	if (language === "node") {
		const apiKey = settings.qdrantKey
			? `,\n  apiKey: ${JSON.stringify(settings.qdrantKey)}`
			: "";
		return `// npm install @veristamp/nqql
import { Client } from "@veristamp/nqql";

const qql = ${JSON.stringify(source)};
const client = new Client({
  url: ${url}${apiKey}
});
const report = await client.execute(qql);
console.log(report);`;
	}

	if (language === "rust") {
		const rustKey = settings.qdrantKey
			? `Some(${JSON.stringify(settings.qdrantKey)}.to_owned())`
			: "None";
		return `// Cargo.toml: qql = "0.1", tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executor = Executor::rest(${url}, ${rustKey})?;
    let report = executor
        .execute(${quoteRust(source)}, OnError::Stop)
        .await?;
    println!("{report:#?}");
    Ok(())
}`;
	}

	if (!route) return "A compiled route is required before exporting cURL.";
	const payload = pretty(route.payload).replace(/'/g, "'\\''");
	const header = settings.qdrantKey
		? ` \\\n  -H ${JSON.stringify(`api-key: ${settings.qdrantKey}`)}`
		: "";
	return `# Statement ${state.selectedStatement + 1} of ${statementCount} · compiled from the editor
curl -X ${route.method} ${JSON.stringify(`${settings.qdrantUrl}${route.path}`)} \\
  -H "content-type: application/json"${header} \\
  --data '${payload}'`;
}

const workspace = required<HTMLElement>("[data-default-query]");
const editorHost = required<HTMLElement>("#qql-editor");
const runButton = required<HTMLButtonElement>("[data-run]");
const formatButton = required<HTMLButtonElement>("[data-format]");
const exportButton = required<HTMLButtonElement>("[data-open-export]");
const validationBadge = required<HTMLElement>("[data-validation-badge]");
const analysisSummary = required<HTMLElement>("[data-analysis-summary]");
const runtimeStatus = required<HTMLElement>("[data-runtime-status]");
const runtimeDot = required<HTMLElement>("[data-runtime-dot]");
const connectionValue = required<HTMLElement>("[data-connection-value]");
const activeFixture = required<HTMLElement>("[data-active-fixture]");
const statementSelect = required<HTMLSelectElement>("[data-statement-select]");
const planEmpty = required<HTMLElement>("[data-empty-plan]");
const planCard = required<HTMLElement>("[data-plan-card]");
const routeMethod = required<HTMLElement>("[data-route-method]");
const routePath = required<HTMLElement>("[data-route-path]");
const routeType = required<HTMLElement>("[data-route-type]");
const routePolicy = required<HTMLElement>("[data-route-policy]");
const routesSection = required<HTMLElement>("[data-routes-section]");
const routesList = required<HTMLElement>("[data-routes-list]");
const routesCount = required<HTMLElement>("[data-routes-count]");
const policyDot = required<HTMLElement>("[data-policy-dot]");
const policyChip = required<HTMLButtonElement>("[data-policy-chip]");
const runLabel = required<HTMLElement>("[data-run-label]");
const toastRegion = required<HTMLElement>("[data-toast-region]");
const embedStatus = required<HTMLElement>("[data-embed-status]");
const embedProbe = required<HTMLElement>("[data-embed-probe]");
const embedDimChip = required<HTMLElement>("[data-embed-dim-chip]");
const responseRich = required<HTMLElement>("[data-response-rich]");
const responsePre = required<HTMLElement>(`[data-output="response"]`);
const editorLoading = required<HTMLElement>("[data-editor-loading]");
const shareButton = required<HTMLButtonElement>("[data-share]");
const docsBacklink = required<HTMLAnchorElement>("[data-docs-backlink]");
const connectedLabel = required<HTMLElement>("[data-connected-label]");
const connectedDot = required<HTMLElement>("[data-connected-dot]");
const statusEndpoint = required<HTMLElement>("[data-status-endpoint]");
const statusEmbed = required<HTMLElement>("[data-status-embed]");
const statusWasm = required<HTMLElement>("[data-status-wasm]");
const statusDetail = required<HTMLElement>("[data-status-detail]");

const state: PlaygroundState = {
	analysis: null,
	response: null,
	executionError: null,
	selectedStatement: 0,
	inspectorTab: "plan",
	exportLanguage: "python",
	metrics: null,
};

let settings = loadStored(SETTINGS_KEY, DEFAULT_SETTINGS);
if (migrateSettings(settings)) {
	try {
		localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
	} catch {
		// Private browsing may deny storage; the session still uses migrated values.
	}
}
let policy = loadStored(POLICY_KEY, DEFAULT_POLICY);
let client: Client | null = null;
let activeExecutions = 0;
const retiredClients = new Set<Client>();
let analysisTimer: number | null = null;

function toast(message: string, tone: "success" | "error" = "success"): void {
	const item = document.createElement("div");
	item.className = `toast toast--${tone}`;
	item.textContent = message;
	toastRegion.append(item);
	window.setTimeout(() => item.remove(), 3600);
}

/** Human sentence plus the parsed error code; raw JSON stays in the tooltip. */
function toastFailure(failure: PlaygroundFailure): void {
	const item = document.createElement("div");
	item.className = "toast toast--error";
	item.title = failure.code ? `${failure.code}: ${failure.raw}` : failure.raw;
	const message = document.createElement("span");
	message.textContent = failure.message;
	item.append(message);
	if (failure.code) {
		item.append(document.createTextNode(" "));
		const code = document.createElement("code");
		code.className =
			"rounded border border-[var(--sl-color-gray-5)] px-1 font-mono text-[0.65rem] whitespace-nowrap text-[var(--sl-color-gray-3)]";
		code.textContent = failure.code;
		item.append(code);
	}
	toastRegion.append(item);
	window.setTimeout(() => item.remove(), 5600);
}

function setRuntime(
	message: string,
	tone: "idle" | "ready" | "failed" = "idle",
): void {
	runtimeStatus.textContent = message;
	runtimeDot.classList.remove("is-ready", "is-failed");
	if (tone === "ready") runtimeDot.classList.add("is-ready");
	if (tone === "failed") runtimeDot.classList.add("is-failed");
}

/** Show "Ctrl" instead of "⌘" in kbd hints when the host is not Apple hardware. */
function applyPlatformKeyHints(): void {
	const isApple = /Mac|iPhone|iPad|iPod/i.test(
		navigator.platform || navigator.userAgent,
	);
	if (isApple) return;
	for (const kbd of all<HTMLElement>("[data-kbd]")) {
		kbd.textContent = (kbd.textContent ?? "").replace("⌘", "Ctrl ");
	}
}

function setEditorLoading(message: string | null): void {
	editorLoading.hidden = message == null;
	if (message != null) editorLoading.textContent = message;
	editorHost.setAttribute("aria-busy", String(message != null));
}

function isSafeDocsRef(ref: string): boolean {
	// Allow only same-origin /docs/* paths. Reject backslashes, control
	// chars, and HTML-significant chars so the value cannot break out of
	// the href context, and re-parse to block encoded // or scheme tricks.
	if (!/^\/docs(?:\/|$)/.test(ref)) return false;
	if (/[\\<>"'`\s]/.test(ref)) return false;
	try {
		const parsed = new URL(ref, window.location.origin);
		if (parsed.origin !== window.location.origin) return false;
		if (
			!(parsed.pathname === "/docs" || parsed.pathname.startsWith("/docs/"))
		) {
			return false;
		}
		return true;
	} catch {
		return false;
	}
}

function setValidationBadge(text: string): void {
	validationBadge.replaceChildren();
	const dot = document.createElement("span");
	dot.className = "status-dot";
	validationBadge.append(dot, document.createTextNode(text));
}

function connectionHost(url: string): string {
	try {
		const parsed = new URL(url);
		return parsed.host || url;
	} catch {
		return url.replace(/^https?:\/\//, "");
	}
}

function updateConnectionSummary(): void {
	const embedding =
		settings.embedProvider === "browser"
			? "MiniLM"
			: settings.embedProvider === "http"
				? settings.embedModel || "HTTP embeddings"
				: "no embedder";
	const summary = `${connectionHost(settings.qdrantUrl)} · ${embedding}`;
	connectionValue.textContent = summary;
	// Full identity lives in the tooltip: connection chips never truncate it.
	const embedIdentity =
		settings.embedProvider === "http"
			? `${settings.embedUrl} · ${settings.embedModel || "(no model)"} · dim ${settings.embedDim}`
			: embedSummary();
	const full = `Qdrant ${settings.qdrantUrl} · embeddings ${embedIdentity}. ${embedBreakage()}`;
	connectionValue.parentElement?.setAttribute("title", full);
	statusEmbed.textContent =
		settings.embedProvider === "http" && settings.embedModel
			? settings.embedModel
			: embedSummary();
	statusEmbed.parentElement?.setAttribute("title", full);
}

function embedSummary(): string {
	if (settings.embedProvider === "browser") return "MiniLM (browser)";
	if (settings.embedProvider === "http") {
		return settings.embedModel
			? `HTTP ${settings.embedModel}`
			: "HTTP embedder";
	}
	return "None (vectors only)";
}

function embedBreakage(): string {
	if (settings.embedProvider === "browser") {
		return "Browser MiniLM downloads on the first TEXT query. TEXT queries fail until the download finishes. Explicit vectors always work.";
	}
	if (settings.embedProvider === "http") {
		return `TEXT queries embed through ${settings.embedUrl || "the configured endpoint"}. Explicit vectors still work when the endpoint is down.`;
	}
	return "TEXT queries fail with no embedder. Use explicit vectors such as [0.1, 0.2, 0.3].";
}

function syncPolicyChip(): void {
	policyDot.classList.toggle("is-active", policy.enabled);
	policyChip.hidden = !policy.enabled;
	const value = required<HTMLElement>("[data-policy-chip-value]");
	value.textContent = policy.enabled
		? `${policy.field} ${policy.op} ${policy.value}`
		: "";
}

function releaseRetiredClients(): void {
	if (activeExecutions !== 0) return;
	for (const retired of retiredClients) retired.free();
	retiredClients.clear();
}

function configureClient(): void {
	const previous = client;
	const next = new Client(settings.qdrantUrl, settings.qdrantKey || null);

	if (settings.embedProvider === "browser") {
		next.setEmbedder(
			createBrowserEmbedder((message) => {
				embedStatus.textContent = message;
				setRuntime(message);
			}),
		);
		embedStatus.textContent =
			"Browser model loads only when execution needs text embeddings.";
	} else if (settings.embedProvider === "http") {
		if (!settings.embedUrl || !settings.embedModel || settings.embedDim < 1) {
			next.free();
			throw new Error(
				"HTTP embeddings need an endpoint, model, and dimension.",
			);
		}
		next.setHttpEmbedder(
			settings.embedUrl,
			settings.embedModel,
			settings.embedDim,
			settings.embedKey || null,
		);
		embedStatus.textContent = `Using ${settings.embedModel} through ${settings.embedUrl}.`;
	} else {
		embedStatus.textContent =
			"Text embedding is disabled; use explicit vectors.";
	}

	client = next;
	if (previous) {
		if (activeExecutions === 0) previous.free();
		else retiredClients.add(previous);
	}
	updateConnectionSummary();
}

function analyzeWithPolicy(source: string): PlaygroundAnalysis {
	const started = performance.now();
	const result = analyze(source);
	let effectiveAst = result.ast;
	let effectiveRoutes = result.routes;
	let policyError: string | null = null;

	if (result.valid && policy.enabled) {
		if (result.statements_count !== 1) {
			policyError =
				"Runtime policy injection accepts exactly one statement. Multi-statement scripts fail closed.";
		} else {
			let statement: Stmt | null = null;
			try {
				statement = new Stmt(source);
				applyRuntimePolicy(statement);
				effectiveAst = [statement.toObject()];
				effectiveRoutes = [statement.compileRoute()];
			} catch (error) {
				policyError = formatError(error);
			} finally {
				statement?.free();
			}
		}
	}

	state.metrics = {
		analyzedAt: new Date().toISOString(),
		parseMs: performance.now() - started,
		executeMs: state.metrics?.executeMs ?? null,
		statements: result.statements_count,
		policyApplied: policy.enabled && !policyError && result.valid,
		embedProvider: settings.embedProvider,
	};

	return { source, result, effectiveAst, effectiveRoutes, policyError };
}

function currentDiagnostic(): Diagnostic[] {
	const analysis = state.analysis;
	if (!analysis) return [];
	const { source, result, policyError } = analysis;
	if (policyError) {
		const firstLine = source.indexOf("\n");
		return [
			{
				from: 0,
				to: Math.max(
					1,
					firstLine === -1 ? Math.min(source.length, 1) : firstLine,
				),
				severity: "error",
				message: policyError,
			},
		];
	}
	if (result.valid) return [];
	const extra = (result as { errors?: NonNullable<typeof result.error>[] })
		.errors;
	const analysisErrors =
		extra && extra.length > 0 ? extra : result.error ? [result.error] : [];
	if (analysisErrors.length === 0) return [];
	return analysisErrors.map((err) => {
		const from =
			err.start == null ? 0 : byteOffsetToPosition(source, err.start);
		const rawTo =
			err.end == null ? from + 1 : byteOffsetToPosition(source, err.end);
		return {
			from: Math.min(from, source.length),
			to: Math.min(Math.max(from + 1, rawTo), source.length),
			severity: "error" as const,
			message: `${err.code}: ${err.message}`,
		};
	});
}

function selectedRoute(): CompiledRoute | null {
	return (
		state.analysis?.effectiveRoutes[state.selectedStatement] ??
		state.analysis?.result.route ??
		null
	);
}

function renderStatementSelect(): void {
	const analysis = state.analysis;
	const count = Math.max(
		analysis?.result.statements_count ?? 0,
		analysis?.effectiveRoutes.length ?? 0,
	);
	statementSelect.replaceChildren();
	for (let index = 0; index < count; index += 1) {
		const option = document.createElement("option");
		option.value = String(index);
		option.textContent = `Statement ${index + 1}`;
		statementSelect.append(option);
	}
	state.selectedStatement = Math.min(
		state.selectedStatement,
		Math.max(0, count - 1),
	);
	statementSelect.value = String(state.selectedStatement);
	statementSelect.disabled = count <= 1;
}

function renderPlan(): void {
	const route = selectedRoute();
	planEmpty.hidden = Boolean(route);
	planCard.hidden = !route;
	if (!route) return;
	routeMethod.textContent = route.method;
	routePath.textContent = route.path;
	routeType.textContent = route.stmt_type;
	routePolicy.textContent = policy.enabled
		? "Trusted predicate injected"
		: "Source only";
}

function selectStatement(index: number): void {
	state.selectedStatement = index;
	statementSelect.value = String(index);
	renderPlan();
	renderOutputs();
	renderRoutes();
}

function renderRoutes(): void {
	const routes = state.analysis?.effectiveRoutes ?? [];
	routesSection.hidden = routes.length === 0;
	routesCount.textContent = `${routes.length} ${routes.length === 1 ? "statement" : "statements"}`;
	routesList.replaceChildren();

	routes.forEach((route, index) => {
		const item = document.createElement("button");
		item.type = "button";
		item.className = "route-row";
		item.setAttribute("role", "option");
		item.setAttribute(
			"aria-selected",
			String(index === state.selectedStatement),
		);
		item.setAttribute(
			"aria-current",
			String(index === state.selectedStatement),
		);
		item.dataset.routeIndex = String(index);

		const indexLabel = document.createElement("span");
		indexLabel.className = "route-row__index";
		indexLabel.textContent = String(index + 1);
		const typeLabel = document.createElement("span");
		typeLabel.className = "route-row__type";
		typeLabel.textContent = route.stmt_type;
		const methodLabel = document.createElement("span");
		methodLabel.className = "route-row__method";
		methodLabel.textContent = route.method;
		const pathLabel = document.createElement("span");
		pathLabel.className = "route-row__path";
		pathLabel.textContent = route.path;

		item.append(indexLabel, typeLabel, methodLabel, pathLabel);
		item.addEventListener("click", () => selectStatement(index));
		routesList.append(item);
	});
}

function prettyRaw(raw: string): string {
	try {
		return JSON.stringify(JSON.parse(raw), null, 2);
	} catch {
		return raw;
	}
}

function renderErrorCard(failure: PlaygroundFailure): HTMLElement {
	const card = document.createElement("div");
	card.className =
		"grid gap-2 rounded-lg border border-[var(--sl-color-gray-5)] border-l-2 border-l-[var(--q-bad)] bg-[var(--sl-color-bg)] p-4";
	card.dataset.errorCard = failure.code ?? "unknown";

	const heading = document.createElement("p");
	heading.className =
		"m-0 font-mono text-[0.6875rem] font-semibold uppercase tracking-[0.12em] text-[var(--q-bad)]";
	heading.textContent = "Run failed";
	card.append(heading);

	const sentence = document.createElement("p");
	sentence.className =
		"m-0 text-[0.8125rem] leading-6 text-[var(--sl-color-gray-1)]";
	sentence.textContent = failure.message;
	card.append(sentence);

	if (failure.code === "QQL-MISSING-USING") {
		const slot = document.createElement("div");
		slot.className = "grid gap-2";
		slot.dataset.usingSlot = "";
		const hint = document.createElement("p");
		hint.className =
			"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		hint.textContent = "Looking up this collection's vectors…";
		slot.append(hint);
		card.append(slot);
		void fillUsingSlot(slot, failure);
	}

	const details = document.createElement("details");
	details.className = "pg-details grid gap-2";
	const summary = document.createElement("summary");
	summary.className =
		"cursor-pointer font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]";
	summary.textContent = failure.code
		? `${failure.code} — technical details`
		: "Technical details";
	details.append(summary);

	const meta = document.createElement("dl");
	meta.className = "m-0 grid gap-1 font-mono text-[0.6875rem]";
	const rows: Array<[string, string]> = [];
	if (failure.code) rows.push(["code", failure.code]);
	if (failure.kind) rows.push(["kind", failure.kind]);
	if (failure.span)
		rows.push(["span", `${failure.span.start}..${failure.span.end}`]);
	for (const [key, value] of Object.entries(failure.fields)) {
		rows.push([`field:${key}`, value]);
	}
	for (const [term, definition] of rows) {
		const row = document.createElement("div");
		row.className = "flex gap-2";
		const dt = document.createElement("dt");
		dt.className = "m-0 shrink-0 text-[var(--sl-color-gray-3)]";
		dt.textContent = term;
		const dd = document.createElement("dd");
		dd.className = "m-0 min-w-0 break-all text-[var(--sl-color-gray-1)]";
		dd.textContent = definition;
		row.append(dt, dd);
		meta.append(row);
	}
	if (rows.length > 0) details.append(meta);

	const raw = document.createElement("pre");
	raw.className =
		"pg-hl m-0 overflow-auto rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem] leading-5 whitespace-pre-wrap break-words text-[var(--sl-color-gray-2)]";
	raw.innerHTML = highlightJson(prettyRaw(failure.raw));
	details.append(raw);
	card.append(details);
	return card;
}

/** One-click USING fix: buttons are user-confirmed, never auto-applied. */
async function fillUsingSlot(
	slot: HTMLElement,
	failure: PlaygroundFailure,
): Promise<void> {
	const collection =
		/Collection '([^']+)'/.exec(failure.raw)?.[1] ??
		/FROM\s+([A-Za-z_][A-Za-z0-9_]*)/i.exec(editor.state.doc.toString())?.[1];
	if (!collection) {
		slot.replaceChildren();
		return;
	}
	let topology: LiveCollectionTopology | null = null;
	try {
		topology = await fetchCollectionTopology(collection);
	} catch {
		topology = null;
	}
	slot.replaceChildren();
	if (!topology || topology.dense.length === 0) {
		const names = /Available vectors: ([^\n"]+)/.exec(failure.raw)?.[1];
		const note = document.createElement("p");
		note.className =
			"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		note.textContent = names
			? `No dense vector found. Available vectors: ${names}. Add USING <name> by hand.`
			: "Couldn't read this collection's vectors. Add USING <name> by hand.";
		slot.append(note);
		return;
	}
	const targets =
		topology.dense.length === 1
			? topology.dense
			: topology.dense.filter((name) => name === "dense");
	const choices = targets.length > 0 ? targets : topology.dense;
	for (const vector of choices) {
		const apply = document.createElement("button");
		apply.type = "button";
		apply.className = "app-action app-action--lg w-fit";
		apply.dataset.usingApply = `${collection} ${vector}`;
		apply.textContent = `Apply USING ${vector} AS DENSE`;
		apply.addEventListener("click", () => applyUsing(collection, vector));
		slot.append(apply);
	}
	if (topology.dense.length > 1 && targets.length === 0) {
		const note = document.createElement("p");
		note.className =
			"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		note.textContent =
			"Several dense vectors exist — pick the one this text should search.";
		slot.prepend(note);
	}
}

/** Insert `USING <vector> AS DENSE` after FROM — explicit user confirmation. */
function applyUsing(collection: string, vector: string): void {
	const source = editor.state.doc.toString();
	const clause = `USING ${vector} AS DENSE`;
	let next: string;
	if (/USING\s/i.test(source)) {
		next = source.replace(/USING\s+\S+(\s+AS\s+\S+)?/i, clause);
	} else {
		const from = new RegExp(`(FROM\\s+${escapeRegExp(collection)}\\s+)`, "i");
		if (!from.test(source)) {
			toast(`Couldn't find FROM ${collection} in the editor.`, "error");
			return;
		}
		next = source.replace(from, `$1${clause} `);
	}
	editor.dispatch({
		changes: { from: 0, to: editor.state.doc.length, insert: next },
	});
	activeFixture.textContent = "Custom query";
	editor.focus();
	toast(`Applied ${clause} — Run again.`);
}

interface ResultHit {
	id: string | number | bigint;
	score?: number;
	payload?: Record<string, unknown> | null;
}

function formatId(id: unknown): string {
	return typeof id === "bigint" ? printableBigInt(id) : String(id);
}

function formatScalar(value: unknown): string {
	if (typeof value === "bigint") return printableBigInt(value);
	if (typeof value === "string") return value;
	if (value == null) return "null";
	if (typeof value === "number" || typeof value === "boolean")
		return String(value);
	const flat = JSON.stringify(value);
	return flat.length > 160 ? `${flat.slice(0, 157)}…` : flat;
}

function isHitArray(data: unknown): data is ResultHit[] {
	return (
		Array.isArray(data) &&
		data.length > 0 &&
		typeof data[0] === "object" &&
		data[0] !== null &&
		"id" in data[0]
	);
}

const PINNED_PAYLOAD_KEYS = [
	"name",
	"room_type",
	"price",
	"rating",
	"neighbourhood",
];

function renderPayloadRows(
	list: HTMLElement,
	payload: Record<string, unknown>,
	capped: boolean,
): void {
	const entries = Object.entries(payload);
	const rank = (key: string): number => {
		const pinned = PINNED_PAYLOAD_KEYS.indexOf(key);
		return pinned === -1 ? PINNED_PAYLOAD_KEYS.length : pinned;
	};
	const ordered = [...entries].sort(([a], [b]) => rank(a) - rank(b));
	const shown = capped ? ordered.slice(0, 12) : ordered;
	for (const [key, value] of shown) {
		const row = document.createElement("div");
		row.className =
			"grid grid-cols-[minmax(6rem,10rem)_minmax(0,1fr)] gap-2 px-2 py-1.5";
		const term = document.createElement("dt");
		term.className =
			"m-0 min-w-0 truncate font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		term.textContent = key;
		term.title = key;
		const definition = document.createElement("dd");
		definition.className =
			"m-0 min-w-0 break-words font-mono text-[0.6875rem] leading-5 text-[var(--sl-color-gray-1)]";
		if (
			key === "description" &&
			typeof value === "string" &&
			value.length > 160
		) {
			definition.className += " line-clamp-3";
		}
		definition.textContent = formatScalar(value);
		if (definition.textContent.length > 160)
			definition.title = formatScalar(value);
		row.append(term, definition);
		list.append(row);
	}
	if (capped && entries.length > shown.length) {
		const more = document.createElement("p");
		more.className =
			"m-0 px-2 py-1 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		more.textContent = `+${entries.length - shown.length} more fields in Raw JSON below.`;
		list.append(more);
	}
}

function renderHitCard(hit: ResultHit): HTMLElement {
	const card = document.createElement("article");
	card.className =
		"grid gap-1.5 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3";
	const header = document.createElement("header");
	header.className = "flex min-w-0 items-center justify-between gap-2";
	const id = document.createElement("code");
	id.className =
		"min-w-0 truncate font-mono text-[0.75rem] font-semibold text-[var(--sl-color-accent-high)]";
	id.textContent = `#${formatId(hit.id)}`;
	id.title = formatId(hit.id);
	header.append(id);
	if (typeof hit.score === "number") {
		const score = document.createElement("span");
		score.className =
			"shrink-0 rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-2 py-0.5 font-mono text-[0.6875rem] text-[var(--sl-color-gray-2)]";
		score.textContent = `score ${hit.score.toFixed(4)}`;
		header.append(score);
	}
	card.append(header);
	if (hit.payload && typeof hit.payload === "object") {
		const list = document.createElement("dl");
		list.className =
			"m-0 grid divide-y divide-[var(--sl-color-gray-5)] rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)]";
		renderPayloadRows(list, hit.payload, true);
		card.append(list);
	}
	return card;
}

function renderResultCard(
	result: { ok: boolean; operation: string; message: string; data: unknown },
	index: number,
): HTMLElement {
	const section = document.createElement("section");
	section.className = "grid content-start gap-2";
	const header = document.createElement("header");
	header.className = "flex min-w-0 flex-wrap items-center gap-2";
	const counter = document.createElement("span");
	counter.className =
		"font-mono text-[0.6875rem] text-[var(--sl-color-gray-4)]";
	counter.textContent = `#${index + 1}`;
	const op = document.createElement("span");
	op.className =
		"rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-7)] px-1.5 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-gray-1)]";
	op.textContent = result.operation;
	const badge = document.createElement("span");
	badge.className = `font-mono text-[0.6875rem] ${result.ok ? "text-[var(--q-ok)]" : "text-[var(--q-bad)]"}`;
	badge.textContent = result.ok ? "ok" : "failed";
	header.append(counter, op, badge);
	if (!result.ok && result.message) {
		const message = document.createElement("span");
		message.className =
			"min-w-0 flex-1 truncate text-xs text-[var(--sl-color-gray-3)]";
		message.textContent = result.message;
		message.title = result.message;
		header.append(message);
	}
	section.append(header);

	const data = result.data;
	if (isHitArray(data)) {
		const count = document.createElement("p");
		count.className =
			"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		count.textContent = `${data.length} ${data.length === 1 ? "hit" : "hits"}`;
		section.append(count);
		for (const hit of data.slice(0, 30)) section.append(renderHitCard(hit));
		if (data.length > 30) {
			const more = document.createElement("p");
			more.className =
				"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
			more.textContent = `Showing 30 of ${data.length} — see Raw JSON for the rest.`;
			section.append(more);
		}
		return section;
	}
	if (
		data &&
		typeof data === "object" &&
		!Array.isArray(data) &&
		typeof (data as { count?: unknown }).count === "number"
	) {
		const count = document.createElement("p");
		count.className =
			"m-0 font-mono text-2xl font-semibold text-[var(--sl-color-white)]";
		count.textContent = String((data as { count: number }).count);
		const label = document.createElement("p");
		label.className =
			"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
		label.textContent = "points counted";
		section.append(count, label);
		return section;
	}
	if (
		Array.isArray(data) &&
		data.every(
			(entry) =>
				entry &&
				typeof entry === "object" &&
				"value" in entry &&
				"count" in entry,
		)
	) {
		const rows = data as Array<{ value: unknown; count: number }>;
		const max = Math.max(1, ...rows.map((row) => row.count));
		const list = document.createElement("div");
		list.className =
			"grid gap-1 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3";
		for (const row of rows.slice(0, 20)) {
			const line = document.createElement("div");
			line.className = "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2";
			const label = document.createElement("span");
			label.className =
				"min-w-0 truncate font-mono text-[0.6875rem] text-[var(--sl-color-gray-1)]";
			label.textContent = formatScalar(row.value);
			label.title = formatScalar(row.value);
			const bar = document.createElement("div");
			bar.className =
				"h-1.5 min-w-16 overflow-hidden rounded-full bg-[var(--sl-color-gray-7)]";
			const fill = document.createElement("div");
			fill.className = "h-full rounded-full bg-[var(--sl-color-accent)]";
			fill.style.width = `${Math.max(4, Math.round((row.count / max) * 100))}%`;
			bar.append(fill);
			const count = document.createElement("span");
			count.className =
				"font-mono text-[0.6875rem] tabular-nums text-[var(--sl-color-gray-3)]";
			count.textContent = String(row.count);
			line.append(label, count);
			list.append(line, bar);
		}
		section.append(list);
		return section;
	}
	if (
		data &&
		typeof data === "object" &&
		!Array.isArray(data) &&
		Array.isArray((data as { groups?: unknown }).groups)
	) {
		const groups = (
			data as { groups: Array<{ id: unknown; hits: ResultHit[] }> }
		).groups;
		for (const group of groups.slice(0, 10)) {
			const block = document.createElement("div");
			block.className =
				"grid gap-1.5 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3";
			const title = document.createElement("p");
			title.className =
				"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
			title.textContent = `group ${formatScalar(group.id)} · ${group.hits?.length ?? 0} hits`;
			block.append(title);
			for (const hit of (group.hits ?? []).slice(0, 5)) {
				const row = document.createElement("p");
				row.className =
					"m-0 flex justify-between gap-2 font-mono text-[0.6875rem]";
				const id = document.createElement("span");
				id.className = "min-w-0 truncate text-[var(--sl-color-accent-high)]";
				id.textContent = `#${formatId(hit.id)}`;
				row.append(id);
				if (typeof hit.score === "number") {
					const score = document.createElement("span");
					score.className =
						"shrink-0 tabular-nums text-[var(--sl-color-gray-3)]";
					score.textContent = hit.score.toFixed(4);
					row.append(score);
				}
				block.append(row);
			}
			section.append(block);
		}
		return section;
	}
	if (result.message) {
		const message = document.createElement("p");
		message.className = "m-0 text-[0.8125rem] text-[var(--sl-color-gray-1)]";
		message.textContent = result.message;
		section.append(message);
	}
	return section;
}

function renderRawDetails(report: unknown): HTMLElement {
	const details = document.createElement("details");
	details.className = "pg-details grid gap-2";
	const summary = document.createElement("summary");
	summary.className =
		"cursor-pointer font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]";
	summary.textContent = "Raw JSON";
	details.append(summary);
	const raw = document.createElement("pre");
	raw.className =
		"pg-hl m-0 overflow-auto rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem] leading-5 whitespace-pre-wrap break-words text-[var(--sl-color-gray-2)]";
	raw.innerHTML = highlightJson(displayJson(report));
	details.append(raw);
	return details;
}

function renderResponse(): void {
	responseRich.replaceChildren();
	const failure = state.executionError;
	if (failure) {
		responseRich.hidden = false;
		responsePre.hidden = true;
		responseRich.append(renderErrorCard(failure));
		return;
	}
	const report = state.response;
	if (!report) {
		responseRich.hidden = true;
		responsePre.hidden = false;
		responsePre.textContent = "Run a valid query to see the Qdrant response.";
		return;
	}
	responseRich.hidden = false;
	responsePre.hidden = true;
	const summary = document.createElement("p");
	summary.className =
		"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]";
	const timing =
		state.metrics?.executeMs != null
			? ` · ${state.metrics.executeMs.toFixed(0)} ms`
			: "";
	summary.textContent = report.ok
		? `${report.succeeded} succeeded${timing}`
		: `${report.succeeded} succeeded · ${report.failed} failed${timing}`;
	responseRich.append(summary);
	report.results.forEach((result, index) => {
		responseRich.append(renderResultCard(result, index));
	});
	responseRich.append(renderRawDetails(report));
}

function renderOutputs(): void {
	const analysis = state.analysis;
	const route = selectedRoute();
	const ast = analysis?.effectiveAst?.[state.selectedStatement] ?? null;

	const values: Record<Exclude<InspectorTab, "plan" | "response">, unknown> = {
		wire: route
			? {
					method: route.method,
					path: route.path,
					payload: route.payload,
				}
			: "No compiled route.",
		ast: ast ?? "No AST output.",
		tokens:
			analysis?.result.tokens.map((token) => ({
				kind: token.kind,
				text: token.text,
				span: [token.pos, token.end],
			})) ?? "No tokens.",
		explain:
			analysis?.result.explain ??
			(analysis?.policyError
				? analysis.policyError
				: "No explanation is available."),
		metrics: state.metrics ?? "Analysis metrics are not available.",
	};

	for (const [name, value] of Object.entries(values)) {
		const pre = required<HTMLElement>(`[data-output="${name}"]`);
		if (name === "explain") renderExplain(pre, value);
		else renderHighlighted(pre, value);
	}
	renderResponse();
}

function renderValidation(): void {
	const analysis = state.analysis;
	const valid = analysis?.result.valid === true && analysis.policyError == null;
	validationBadge.classList.remove("is-loading", "is-valid", "is-invalid");
	validationBadge.classList.add(valid ? "is-valid" : "is-invalid");

	if (!analysis) {
		validationBadge.textContent = "WASM unavailable";
		analysisSummary.textContent =
			"The current qql-wasm package could not load.";
	} else if (analysis.policyError) {
		setValidationBadge("Policy blocked");
		analysisSummary.textContent = analysis.policyError;
	} else if (analysis.result.valid) {
		setValidationBadge("Valid QQL");
		analysisSummary.textContent = `${analysis.result.statements_count} ${
			analysis.result.statements_count === 1 ? "statement" : "statements"
		}, ${analysis.result.tokens.length} tokens, ${state.metrics?.parseMs.toFixed(2)} ms`;
	} else {
		setValidationBadge("Invalid QQL");
		// analyze reports the first error only (a single `error` field), so
		// render its stable code with a link to the error code reference.
		const code = analysis.result.error?.code;
		const message =
			analysis.result.error?.message ?? "The parser rejected this input.";
		if (code) {
			analysisSummary.replaceChildren();
			const link = document.createElement("a");
			link.href = "/docs/reference/error-codes";
			link.textContent = code;
			link.title = "Open the error code reference";
			analysisSummary.append(link, document.createTextNode(`: ${message}`));
		} else {
			analysisSummary.textContent = message;
		}
	}

	runButton.disabled = !valid;
	exportButton.disabled = !valid;
}

function renderInspector(): void {
	renderStatementSelect();
	renderPlan();
	renderOutputs();
	renderRoutes();
	renderValidation();
}

function formatEditor(): void {
	const source = editor.state.doc.toString();
	try {
		const formatted = formatQuery(source);
		if (formatted === source) {
			toast("Already formatted.");
			return;
		}
		editor.dispatch({
			changes: { from: 0, to: editor.state.doc.length, insert: formatted },
		});
		editor.focus();
		toast("Formatted with the canonical formatter.");
	} catch (error) {
		toast(formatError(error), "error");
	}
}

function runAnalysis(source: string): void {
	try {
		state.analysis = analyzeWithPolicy(source);
	} catch (error) {
		const message = formatError(error);
		// The synthetic fallback must satisfy the current AnalysisResult shape:
		// `errors` is the primary diagnostic list; `error` is kept for older clients.
		const diagnostic = {
			code: "QQL-WASM",
			message,
			start: null,
			end: null,
		};
		state.analysis = {
			source,
			result: {
				valid: false,
				statements_count: 0,
				tokens: [],
				ast: null,
				route: null,
				routes: [],
				explain: null,
				error: diagnostic,
				errors: [diagnostic],
			},
			effectiveAst: null,
			effectiveRoutes: [],
			policyError: null,
		};
	}
	state.selectedStatement = 0;
	renderInspector();
	forceLinting(editor);
}

function queueAnalysis(source: string): void {
	if (analysisTimer != null) window.clearTimeout(analysisTimer);
	analysisTimer = window.setTimeout(() => {
		analysisTimer = null;
		runAnalysis(source);
	}, ANALYSIS_DELAY_MS);
}

function switchInspectorTab(tab: InspectorTab, focus = false): void {
	state.inspectorTab = tab;
	saveSession(INSPECTOR_TAB_KEY, tab);
	for (const button of all<HTMLButtonElement>("[data-inspector-tab]")) {
		const active = button.dataset.inspectorTab === tab;
		button.setAttribute("aria-selected", String(active));
		button.tabIndex = active ? 0 : -1;
		if (active && focus) button.focus();
	}
	for (const panel of all<HTMLElement>("[data-inspector-panel]")) {
		panel.hidden = panel.dataset.inspectorPanel !== tab;
	}
}

function setupTabs(): void {
	const tabs = all<HTMLButtonElement>("[data-inspector-tab]");
	tabs.forEach((button, index) => {
		button.addEventListener("click", () => {
			switchInspectorTab(button.dataset.inspectorTab as InspectorTab);
		});
		button.addEventListener("keydown", (event) => {
			if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
			event.preventDefault();
			const direction = event.key === "ArrowRight" ? 1 : -1;
			const next = (index + direction + tabs.length) % tabs.length;
			switchInspectorTab(tabs[next].dataset.inspectorTab as InspectorTab, true);
		});
	});
}

function openDialog(id: string): void {
	const dialog = required<HTMLDialogElement>(id);
	dialog.showModal();
	if (id === "#preset-dialog") {
		window.setTimeout(
			() => required<HTMLInputElement>("[data-preset-search]").focus(),
			0,
		);
	}
}

function setupDialogs(): void {
	for (const dialog of all<HTMLDialogElement>("dialog[data-app-dialog]")) {
		dialog.addEventListener("click", (event) => {
			if (event.target === dialog) dialog.close();
		});
		for (const close of all<HTMLButtonElement>(
			`#${dialog.id} [data-close-dialog]`,
		)) {
			close.addEventListener("click", () => dialog.close());
		}
	}
	for (const button of all<HTMLElement>("[data-open-presets]")) {
		button.addEventListener("click", () => openDialog("#preset-dialog"));
	}
	for (const button of all<HTMLElement>("[data-open-settings]")) {
		button.addEventListener("click", () => openDialog("#settings-dialog"));
	}
	required("[data-open-policy]").addEventListener("click", () =>
		openDialog("#policy-dialog"),
	);
	policyChip.addEventListener("click", () => openDialog("#policy-dialog"));
	exportButton.addEventListener("click", () => {
		renderExport();
		openDialog("#export-dialog");
	});
}

function setupPresets(): void {
	const search = required<HTMLInputElement>("[data-preset-search]");
	const results = required<HTMLElement>("[data-preset-results]");
	const cards = all<HTMLButtonElement>("[data-preset]");
	const categories = all<HTMLButtonElement>("[data-preset-category]");
	let category = "all";

	const filter = () => {
		const query = search.value.trim().toLowerCase();
		for (const card of cards) {
			const categoryMatches =
				category === "all" || card.dataset.presetCategoryValue === category;
			const text = [
				card.dataset.presetLabel,
				card.dataset.presetDescription,
				card.dataset.presetId,
			]
				.join(" ")
				.toLowerCase();
			card.hidden = !categoryMatches || !text.includes(query);
		}
		const shown = cards.filter((card) => !card.hidden).length;
		results.textContent = `${shown} of ${cards.length} fixture examples shown`;
	};

	search.addEventListener("input", filter);
	for (const button of categories) {
		button.addEventListener("click", () => {
			category = button.dataset.presetCategory ?? "all";
			categories.forEach((item) => {
				item.setAttribute("aria-pressed", String(item === button));
			});
			filter();
		});
	}
	for (const card of cards) {
		card.addEventListener("click", () => {
			const query = card.dataset.presetQuery ?? "";
			editor.dispatch({
				changes: { from: 0, to: editor.state.doc.length, insert: query },
			});
			activeFixture.textContent = card.dataset.presetLabel ?? "Custom query";
			required<HTMLDialogElement>("#preset-dialog").close();
			editor.focus();
		});
		card.addEventListener("keydown", (event) => {
			const visible = cards.filter((item) => !item.hidden);
			const index = visible.indexOf(card);
			if (index === -1) return;
			let next: number | null = null;
			if (event.key === "Home") next = 0;
			if (event.key === "End") next = visible.length - 1;
			if (event.key === "ArrowRight" || event.key === "ArrowDown")
				next = index + 1;
			if (event.key === "ArrowLeft" || event.key === "ArrowUp")
				next = index - 1;
			if (next == null) return;
			event.preventDefault();
			visible[(next + visible.length) % visible.length]?.focus();
		});
	}
}

function currentShareUrl(): URL {
	const url = new URL(window.location.href);
	url.search = "";
	url.hash = "";
	url.searchParams.set("q", editor.state.doc.toString());
	const ref = docsBacklink.hidden ? null : docsBacklink.getAttribute("href");
	if (ref && isSafeDocsRef(ref)) url.searchParams.set("ref", ref);
	return url;
}

function shareHeadline(source: string): string {
	const line =
		source
			.split("\n")
			.map((row) => row.trim())
			.find((row) => row && !row.startsWith("--")) ?? "a QQL query";
	const clipped = line.length > 72 ? `${line.slice(0, 69)}…` : line;
	return `Try this in the QQL playground:\n${clipped}`;
}

function positionShareMenu(menu: HTMLElement): void {
	const rect = shareButton.getBoundingClientRect();
	const width = menu.offsetWidth;
	const left = Math.min(
		Math.max(8, rect.right - width),
		window.innerWidth - width - 8,
	);
	menu.style.top = `${rect.bottom + 8}px`;
	menu.style.left = `${left}px`;
}

function setupShare(): void {
	const menu = required<HTMLElement>("[data-share-menu]");
	const urlInput = required<HTMLInputElement>("[data-share-url]");
	const copyButton = required<HTMLButtonElement>("[data-share-copy]");
	const copyLabel = required<HTMLElement>("[data-share-copy-label]");
	const xLink = required<HTMLAnchorElement>("[data-share-x]");
	const linkedinLink = required<HTMLAnchorElement>("[data-share-linkedin]");
	const nativeButton = document.querySelector<HTMLButtonElement>(
		"[data-share-native]",
	);
	const canPopover = "showPopover" in HTMLElement.prototype;
	let copiedTimer = 0;

	const fill = (): string => {
		const url = currentShareUrl().toString();
		const text = shareHeadline(editor.state.doc.toString());
		urlInput.value = url;
		xLink.href = `https://x.com/intent/tweet?text=${encodeURIComponent(text)}&url=${encodeURIComponent(url)}`;
		linkedinLink.href = `https://www.linkedin.com/sharing/share-offsite/?url=${encodeURIComponent(url)}`;
		return url;
	};

	const markCopied = (copied: boolean): void => {
		if (copied) copyButton.dataset.copied = "true";
		else delete copyButton.dataset.copied;
		copyLabel.textContent = copied ? "Copied" : "Copy";
		copyButton.setAttribute("aria-label", copied ? "Link copied" : "Copy link");
	};

	const copyShareUrl = async (): Promise<void> => {
		const url = fill();
		try {
			await navigator.clipboard.writeText(url);
			markCopied(true);
			toast("Share link copied.");
			window.clearTimeout(copiedTimer);
			copiedTimer = window.setTimeout(() => markCopied(false), 2000);
		} catch {
			urlInput.select();
			toast(
				"Clipboard access was denied. Copy the URL from the field.",
				"error",
			);
		}
	};

	if (nativeButton && "share" in navigator) {
		nativeButton.hidden = false;
		nativeButton.addEventListener("click", async () => {
			const url = fill();
			try {
				await navigator.share({
					title: "QQL Playground",
					text: shareHeadline(editor.state.doc.toString()),
					url,
				});
			} catch (error) {
				if (error instanceof Error && error.name === "AbortError") return;
				toast("Share failed.", "error");
			}
		});
	}

	if (!canPopover) {
		shareButton.addEventListener("click", () => void copyShareUrl());
		return;
	}

	menu.addEventListener("toggle", () => {
		const open = menu.matches(":popover-open");
		shareButton.setAttribute("aria-expanded", String(open));
		if (!open) {
			markCopied(false);
			return;
		}
		fill();
		positionShareMenu(menu);
		urlInput.focus();
		urlInput.select();
	});

	window.addEventListener("resize", () => {
		if (menu.matches(":popover-open")) positionShareMenu(menu);
	});

	copyButton.addEventListener("click", () => void copyShareUrl());
	urlInput.addEventListener("focus", () => urlInput.select());
}

function setupDocsBacklink(ref: string | null): void {
	if (!ref || !isSafeDocsRef(ref)) return;
	// Rebuild the link from the parsed, same-origin URL instead of the raw
	// parameter; encodeURI is the final barrier so no scheme, authority, or
	// markup can reach the DOM. For same-origin /docs paths this is
	// byte-identical to `ref` (encodeURI preserves / ? & = : #).
	const target = new URL(ref, window.location.origin);
	docsBacklink.setAttribute("href", encodeURI(target.pathname + target.search));
	docsBacklink.hidden = false;
}

function writeSettingsForm(): void {
	const form = required<HTMLFormElement>("[data-settings-form]");
	const field = <T extends HTMLInputElement | HTMLSelectElement>(
		name: string,
	) => required<T>(`[data-settings-form] [name="${name}"]`);
	field<HTMLInputElement>("qdrantUrl").value = settings.qdrantUrl;
	field<HTMLInputElement>("qdrantKey").value = settings.qdrantKey;
	field<HTMLSelectElement>("embedProvider").value = settings.embedProvider;
	field<HTMLInputElement>("embedUrl").value = settings.embedUrl;
	field<HTMLInputElement>("embedModel").value = settings.embedModel;
	field<HTMLInputElement>("embedDim").value = String(settings.embedDim);
	field<HTMLInputElement>("embedKey").value = settings.embedKey;

	const toggleHttpFields = () => {
		required<HTMLFieldSetElement>("[data-http-embed-fields]").hidden =
			field<HTMLSelectElement>("embedProvider").value !== "http";
	};
	field<HTMLSelectElement>("embedProvider").addEventListener(
		"change",
		toggleHttpFields,
	);
	toggleHttpFields();

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		settings = {
			qdrantUrl: field<HTMLInputElement>("qdrantUrl").value.replace(/\/+$/, ""),
			qdrantKey: field<HTMLInputElement>("qdrantKey").value,
			embedProvider: field<HTMLSelectElement>("embedProvider")
				.value as PlaygroundSettings["embedProvider"],
			embedUrl: field<HTMLInputElement>("embedUrl").value,
			embedModel: field<HTMLInputElement>("embedModel").value,
			embedDim: Number(field<HTMLInputElement>("embedDim").value),
			embedKey: field<HTMLInputElement>("embedKey").value,
		};
		try {
			configureClient();
			localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
			required<HTMLDialogElement>("#settings-dialog").close();
			topologyCache.clear();
			runAnalysis(editor.state.doc.toString());
			void checkEndpoint();
			void loadLivePresets();
			toast("Connection settings saved.");
		} catch (error) {
			toast(formatError(error), "error");
		}
	});
	required<HTMLButtonElement>("[data-test-connection]").addEventListener(
		"click",
		() => void checkEndpoint(true),
	);
}

function writePolicyForm(): void {
	const form = required<HTMLFormElement>("[data-policy-form]");
	const field = <T extends HTMLInputElement | HTMLSelectElement>(
		name: string,
	) => required<T>(`[data-policy-form] [name="${name}"]`);
	const recipes = all<HTMLButtonElement>("[data-policy-recipe]");
	field<HTMLInputElement>("enabled").checked = policy.enabled;
	field<HTMLInputElement>("field").value = policy.field;
	field<HTMLSelectElement>("op").value = policy.op;
	field<HTMLInputElement>("value").value = policy.value;
	field<HTMLSelectElement>("valueType").value = policy.valueType;
	field<HTMLInputElement>("shardKey").value = policy.shardKey;
	syncPolicyChip();

	const preview = required<HTMLOutputElement>("[data-policy-preview]");
	const quotePreviewValue = (raw: string, valueType: string) => {
		if (valueType === "string") return `'${raw || "value"}'`;
		return raw || "value";
	};
	const syncRecipeSelection = () => {
		const current = {
			field: field<HTMLInputElement>("field").value.trim(),
			op: field<HTMLSelectElement>("op").value,
			value: field<HTMLInputElement>("value").value,
			valueType: field<HTMLSelectElement>("valueType").value,
			shard: field<HTMLInputElement>("shardKey").value.trim(),
		};
		for (const recipe of recipes) {
			const matches =
				recipe.dataset.field === current.field &&
				(recipe.dataset.op ?? "=") === current.op &&
				recipe.dataset.value === current.value &&
				(recipe.dataset.valueType ?? "string") === current.valueType &&
				(recipe.dataset.shard ?? "") === current.shard;
			recipe.setAttribute("aria-pressed", String(matches));
		}
	};
	const updatePreview = () => {
		const enabled = field<HTMLInputElement>("enabled").checked;
		const fieldName = field<HTMLInputElement>("field").value.trim() || "field";
		const operator = field<HTMLSelectElement>("op").value;
		const valueType = field<HTMLSelectElement>("valueType").value;
		const value = quotePreviewValue(
			field<HTMLInputElement>("value").value,
			valueType,
		);
		const shard = field<HTMLInputElement>("shardKey").value.trim();
		if (!enabled) {
			preview.textContent =
				"The source stays unchanged. Enable injection to rewrite the AST after parse.";
			syncRecipeSelection();
			return;
		}
		const parts = [`WHERE ${fieldName} ${operator} ${value}`];
		if (shard) parts.push(`SHARD '${shard}'`);
		preview.textContent = parts.join("\n");
		syncRecipeSelection();
	};
	updatePreview();

	for (const name of [
		"enabled",
		"field",
		"op",
		"value",
		"valueType",
		"shardKey",
	] as const) {
		field<HTMLInputElement | HTMLSelectElement>(name).addEventListener(
			"input",
			updatePreview,
		);
		field<HTMLInputElement | HTMLSelectElement>(name).addEventListener(
			"change",
			updatePreview,
		);
	}

	for (const recipe of recipes) {
		recipe.addEventListener("click", () => {
			field<HTMLInputElement>("enabled").checked = true;
			field<HTMLInputElement>("field").value = recipe.dataset.field ?? "";
			field<HTMLSelectElement>("op").value = recipe.dataset.op ?? "=";
			field<HTMLInputElement>("value").value = recipe.dataset.value ?? "";
			field<HTMLSelectElement>("valueType").value =
				recipe.dataset.valueType ?? "string";
			field<HTMLInputElement>("shardKey").value = recipe.dataset.shard ?? "";
			updatePreview();
		});
	}

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		policy = {
			enabled: field<HTMLInputElement>("enabled").checked,
			field: field<HTMLInputElement>("field").value.trim(),
			op: field<HTMLSelectElement>("op").value,
			value: field<HTMLInputElement>("value").value,
			valueType: field<HTMLSelectElement>("valueType").value as PolicyValueType,
			shardKey: field<HTMLInputElement>("shardKey").value,
		};
		localStorage.setItem(POLICY_KEY, JSON.stringify(policy));
		syncPolicyChip();
		required<HTMLDialogElement>("#policy-dialog").close();
		runAnalysis(editor.state.doc.toString());
		toast(
			policy.enabled ? "Runtime policy applied." : "Runtime policy disabled.",
		);
	});
}

function renderExport(): void {
	const source = editor.state.doc.toString();
	required<HTMLElement>("[data-export-output]").textContent = exportCode(
		state.exportLanguage,
		source,
		settings,
		selectedRoute(),
		state.analysis?.result.statements_count ?? 1,
	);
}

function setupExporter(): void {
	const tabs = all<HTMLButtonElement>("[data-export-tab]");
	for (const tab of tabs) {
		tab.addEventListener("click", () => {
			state.exportLanguage = tab.dataset.exportTab as ExportLanguage;
			tabs.forEach((item) => {
				item.setAttribute("aria-pressed", String(item === tab));
			});
			renderExport();
		});
	}
	required("[data-copy-export]").addEventListener("click", async () => {
		const code =
			required<HTMLElement>("[data-export-output]").textContent ?? "";
		try {
			await navigator.clipboard.writeText(code);
			toast("SDK code copied.");
		} catch {
			toast("Clipboard access was denied.", "error");
		}
	});
}

async function executeQuery(): Promise<void> {
	if (!client || !state.analysis?.result.valid || state.analysis.policyError) {
		return;
	}
	runButton.disabled = true;
	runButton.classList.add("is-running");
	runLabel.textContent = "Running…";
	state.executionError = null;
	state.response = null;
	renderOutputs();
	setRuntime("Executing against Qdrant…");

	const executionClient = client;
	const source = editor.state.doc.toString();
	const started = performance.now();
	let statement: Stmt | null = null;
	activeExecutions += 1;

	try {
		if (policy.enabled) {
			statement = new Stmt(source);
			applyRuntimePolicy(statement);
			state.response = await executionClient.executeStmt(statement);
		} else {
			state.response = await executionClient.execute(source);
		}
		if (state.metrics) state.metrics.executeMs = performance.now() - started;
		setRuntime(
			state.response.ok
				? `Execution complete, ${state.response.succeeded} succeeded`
				: `Execution complete, ${state.response.failed} failed`,
			state.response.ok ? "ready" : "failed",
		);
		switchInspectorTab("response");
		toast(
			state.response.ok ? "Query executed." : "Qdrant returned a failure.",
			state.response.ok ? "success" : "error",
		);
	} catch (error) {
		state.executionError = buildFailure(error, settings.qdrantUrl);
		if (state.metrics) state.metrics.executeMs = performance.now() - started;
		setRuntime("Execution failed.", "failed");
		switchInspectorTab("response");
		toastFailure(state.executionError);
	} finally {
		statement?.free();
		activeExecutions -= 1;
		releaseRetiredClients();
		runButton.classList.remove("is-running");
		runLabel.textContent = "Run";
		renderValidation();
		renderOutputs();
	}
}

const topologyCache = new Map<string, LiveCollectionTopology>();

function qdrantHeaders(): Record<string, string> {
	return settings.qdrantKey ? { "api-key": settings.qdrantKey } : {};
}

/** Live vector topology for USING suggestions and presets (cached per session). */
async function fetchCollectionTopology(
	collection: string,
): Promise<LiveCollectionTopology> {
	const cached = topologyCache.get(collection);
	if (cached) return cached;
	const base = settings.qdrantUrl.replace(/\/+$/, "");
	const response = await fetch(
		`${base}/collections/${encodeURIComponent(collection)}`,
		{ headers: qdrantHeaders() },
	);
	if (!response.ok) {
		throw new Error(
			`Collection '${collection}' answered HTTP ${response.status}.`,
		);
	}
	const body = (await response.json()) as {
		result?: {
			config?: { params?: { vectors?: unknown; sparse_vectors?: unknown } };
		};
	};
	const params = body?.result?.config?.params ?? {};
	const vectors = params.vectors as Record<
		string,
		{ multivector_config?: unknown }
	>;
	const sparse = params.sparse_vectors as Record<string, unknown> | undefined;
	const dense: string[] = [];
	const multi: string[] = [];
	if (vectors && typeof vectors === "object") {
		for (const [name, config] of Object.entries(vectors)) {
			if (
				config &&
				typeof config === "object" &&
				"multivector_config" in config
			) {
				multi.push(name);
			} else {
				dense.push(name);
			}
		}
	}
	const topology: LiveCollectionTopology = {
		name: collection,
		dense,
		sparse: sparse && typeof sparse === "object" ? Object.keys(sparse) : [],
		multi,
	};
	topologyCache.set(collection, topology);
	return topology;
}

function applyPreset(query: string, label: string): void {
	editor.dispatch({
		changes: { from: 0, to: editor.state.doc.length, insert: query },
	});
	activeFixture.textContent = label;
	required<HTMLDialogElement>("#preset-dialog").close();
	editor.focus();
}

/** One-click presets derived from live GET /collections, USING prefilled. */
async function loadLivePresets(): Promise<void> {
	const list = required<HTMLElement>("[data-live-presets]");
	const hint = required<HTMLElement>("[data-live-hint]");
	list.replaceChildren();
	try {
		const base = settings.qdrantUrl.replace(/\/+$/, "");
		const controller = new AbortController();
		const timer = window.setTimeout(() => controller.abort(), 8000);
		let response: Response;
		try {
			response = await fetch(`${base}/collections`, {
				headers: qdrantHeaders(),
				signal: controller.signal,
			});
		} finally {
			window.clearTimeout(timer);
		}
		if (!response.ok) throw new Error(`HTTP ${response.status}`);
		const body = (await response.json()) as {
			result?: { collections?: Array<{ name: string }> };
		};
		const names = body?.result?.collections?.map((entry) => entry.name) ?? [];
		if (names.length === 0) {
			hint.textContent = "No collections on this Qdrant yet.";
			return;
		}
		const topologies = await Promise.all(
			names.slice(0, 6).map((name) =>
				fetchCollectionTopology(name).catch(
					(): LiveCollectionTopology => ({
						name,
						dense: [],
						sparse: [],
						multi: [],
					}),
				),
			),
		);
		let shown = 0;
		for (const topology of topologies) {
			if (shown >= 4) break;
			const card = buildLivePresetCard(topology);
			if (card) {
				list.append(card);
				shown += 1;
			}
		}
		hint.textContent = `${names.length} ${names.length === 1 ? "collection" : "collections"} on ${connectionHost(base)} — presets prefill USING.`;
	} catch {
		hint.textContent =
			"Qdrant unreachable — live presets need a running Qdrant.";
	}
}

function buildLivePresetCard(
	topology: LiveCollectionTopology,
): HTMLButtonElement | null {
	let label: string;
	let description: string;
	let query: string;
	if (topology.dense.length > 0) {
		const vector = topology.dense.includes("dense")
			? "dense"
			: topology.dense[0];
		label = `${topology.name} — text search`;
		description = `TEXT query with USING ${vector} AS DENSE prefilled.`;
		query = `QUERY 'example search' FROM ${topology.name} USING ${vector} AS DENSE LIMIT 5;`;
	} else {
		label = `${topology.name} — count`;
		description = "No dense vector here; COUNT needs no USING.";
		query = `COUNT FROM ${topology.name};`;
	}
	const card = document.createElement("button");
	card.type = "button";
	card.className =
		"grid content-start gap-1 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-3 text-left text-[var(--sl-color-gray-2)] transition-colors hover:border-[var(--sl-color-gray-4)] hover:bg-[var(--sl-color-bg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--sl-color-accent)]";
	card.dataset.livePreset = topology.name;
	const kicker = document.createElement("span");
	kicker.className =
		"font-mono text-[0.625rem] uppercase tracking-[0.08em] text-[var(--sl-color-gray-3)]";
	kicker.textContent = "Live";
	const title = document.createElement("strong");
	title.className =
		"text-[0.8125rem] font-semibold text-[var(--sl-color-white)]";
	title.textContent = label;
	const hint = document.createElement("span");
	hint.className =
		"line-clamp-2 text-xs leading-5 text-[var(--sl-color-gray-3)]";
	hint.textContent = description;
	card.append(kicker, title, hint);
	card.addEventListener("click", () => applyPreset(query, label));
	return card;
}

async function checkEndpoint(manual = false): Promise<void> {
	const base = settings.qdrantUrl.replace(/\/+$/, "");
	let qdrantOk = false;
	let qdrantLine = "Qdrant check did not finish.";
	statusEndpoint.textContent = "Checking";
	connectedLabel.textContent = "Connected: checking";
	connectedDot.classList.remove("is-ready", "is-failed");
	statusDetail.hidden = true;
	statusDetail.textContent = "";

	const withTimeout = async (init: RequestInit): Promise<Response> => {
		const controller = new AbortController();
		const timer = window.setTimeout(() => controller.abort(), 5000);
		try {
			return await fetch(`${base}/collections`, {
				...init,
				signal: controller.signal,
			});
		} finally {
			window.clearTimeout(timer);
		}
	};

	try {
		const probe = await withTimeout({});
		if (!probe.ok) {
			statusEndpoint.textContent = `Reachable, HTTP ${probe.status}`;
			statusDetail.textContent = `Qdrant answered but refused the request (HTTP ${probe.status}). Set the API key in Connection settings when the instance requires one. Offline analyze still works.`;
			statusDetail.hidden = false;
			connectedLabel.textContent = "Connected: Qdrant reachable";
			connectedDot.classList.add("is-ready");
			qdrantLine = statusDetail.textContent;
		} else {
			statusEndpoint.textContent = `Reachable (${connectionHost(base)})`;
			connectedLabel.textContent = "Connected: Qdrant reachable";
			connectedDot.classList.add("is-ready");
			qdrantLine = "Qdrant is reachable.";
			qdrantOk = true;
		}
	} catch {
		// A normal fetch rejects for both a down server and a CORS block.
		// An opaque no-cors probe resolves only when the server is up, so it
		// tells the two cases apart without extra requests on success.
		let corsBlocked = false;
		try {
			const opaque = await withTimeout({ mode: "no-cors" });
			corsBlocked = opaque.type === "opaque";
		} catch {
			corsBlocked = false;
		}
		if (corsBlocked) {
			statusEndpoint.textContent = "Blocked by browser CORS policy";
			statusDetail.textContent = `Your Qdrant answered at ${base}, but the browser blocked the response. Allow this page origin in the Qdrant CORS settings, then reload. Offline analyze still works.`;
			connectedLabel.textContent = "Connected: blocked by CORS";
		} else {
			statusEndpoint.textContent = `Not reachable (${connectionHost(base)})`;
			statusDetail.textContent = `Qdrant is not reachable at ${base}. Start Qdrant locally with the default REST port 6333 and reload. Offline analyze still works without it.`;
			connectedLabel.textContent = "Connected: Qdrant unreachable";
		}
		statusDetail.hidden = false;
		connectedDot.classList.add("is-failed");
		qdrantLine = statusDetail.textContent;
	}

	// The embedder probe is independent: an embedder outage never fails the
	// Qdrant verdict above, and each side is reported on its own channel.
	const embedder = await probeEmbedder();
	if (manual) {
		toast(
			`${qdrantLine} · ${embedder.line}`,
			qdrantOk && embedder.ok ? "success" : "error",
		);
	}
}

/**
 * POST-probe the configured HTTP embedder (model + dimension check).
 * Reports only through the embedder channels; Qdrant status is untouched.
 */
async function probeEmbedder(): Promise<{ ok: boolean; line: string }> {
	embedDimChip.hidden = true;
	if (settings.embedProvider === "browser") {
		embedProbe.textContent =
			"Browser MiniLM loads on the first TEXT query — no endpoint to probe.";
		return { ok: true, line: "browser embedder (loads on first TEXT query)" };
	}
	if (settings.embedProvider === "none") {
		embedProbe.textContent = "Embeddings disabled — use explicit vectors.";
		return { ok: true, line: "no embedder (vectors only)" };
	}
	embedProbe.textContent = `Probing ${settings.embedUrl}…`;
	try {
		const controller = new AbortController();
		const timer = window.setTimeout(() => controller.abort(), 8000);
		let response: Response;
		try {
			response = await fetch(settings.embedUrl, {
				method: "POST",
				headers: {
					"content-type": "application/json",
					...(settings.embedKey
						? { authorization: `Bearer ${settings.embedKey}` }
						: {}),
				},
				body: JSON.stringify({
					model: settings.embedModel,
					input: ["connection probe"],
				}),
				signal: controller.signal,
			});
		} finally {
			window.clearTimeout(timer);
		}
		if (!response.ok) {
			const line = `Embedder HTTP ${response.status} at ${connectionHost(settings.embedUrl)} — TEXT queries fail, vectors still work.`;
			embedProbe.textContent = line;
			return { ok: false, line };
		}
		const body = (await response.json()) as {
			data?: Array<{ embedding?: number[] }>;
		};
		const dim = body?.data?.[0]?.embedding?.length ?? 0;
		if (!dim) {
			const line =
				"Embedder answered but returned no vector — TEXT queries will fail.";
			embedProbe.textContent = line;
			return { ok: false, line };
		}
		if (dim === settings.embedDim) {
			embedDimChip.hidden = false;
			embedDimChip.textContent = `dim ${dim} ✓`;
			embedDimChip.title = `${settings.embedModel} returned dim ${dim}, as configured.`;
			embedProbe.textContent = `Embedder OK — ${settings.embedModel}, dim ${dim} ✓.`;
			return { ok: true, line: `embedder dim ${dim} ✓` };
		}
		embedDimChip.hidden = false;
		embedDimChip.textContent = `dim ${dim} ≠ ${settings.embedDim}`;
		embedDimChip.title = `Endpoint returned dim ${dim} but Dimension is ${settings.embedDim}.`;
		const line = `Embedder dim ${dim} ≠ configured ${settings.embedDim} — set Dimension to ${dim}.`;
		embedProbe.textContent = line;
		return { ok: false, line };
	} catch {
		const line = `Embedder unreachable at ${connectionHost(settings.embedUrl)} — TEXT queries fail, vectors still work.`;
		embedProbe.textContent = line;
		return { ok: false, line };
	}
}

const wrapCompartment = new Compartment();

function isWrapOn(): boolean {
	try {
		return localStorage.getItem(WRAP_KEY) === "on";
	} catch {
		return false;
	}
}

const pageParams = new URLSearchParams(window.location.search);
const urlQuery = pageParams.get("q");
const initialQuery =
	urlQuery ||
	loadSession(WORKSPACE_KEY) ||
	workspace.dataset.defaultQuery ||
	"";

const editor = new EditorView({
	parent: editorHost,
	state: EditorState.create({
		doc: initialQuery,
		extensions: [
			basicSetup,
			qqlLanguage,
			qqlHighlighting,
			qqlCompletion,
			lintGutter(),
			linter(currentDiagnostic),
			EditorView.contentAttributes.of({
				spellcheck: "false",
				autocorrect: "off",
				autocapitalize: "off",
				translate: "no",
			}),
			keymap.of([
				indentWithTab,
				{
					key: "Mod-Enter",
					run: () => {
						void executeQuery();
						return true;
					},
				},
				{
					key: "Mod-k",
					run: () => {
						openDialog("#preset-dialog");
						return true;
					},
				},
			]),
			wrapCompartment.of(isWrapOn() ? EditorView.lineWrapping : []),
			placeholder(
				"QUERY 'cozy apartment near Mitte' FROM geosmart_berlin_stays USING dense AS DENSE LIMIT 5;\n-- Nothing leaves this tab until you run.",
			),
			EditorView.updateListener.of((update) => {
				if (!update.docChanged) return;
				activeFixture.textContent = "Custom query";
				saveSession(WORKSPACE_KEY, update.state.doc.toString());
				queueAnalysis(update.state.doc.toString());
			}),
		],
	}),
});

/** Wrap toggle: CodeMirror compartment + data-wrap for the author CSS rule. */
function setupWrap(): void {
	const toggle = required<HTMLButtonElement>("[data-wrap-toggle]");
	const apply = (on: boolean): void => {
		editor.dispatch({
			effects: wrapCompartment.reconfigure(on ? EditorView.lineWrapping : []),
		});
		editorHost.dataset.wrap = on ? "on" : "off";
		toggle.setAttribute("aria-pressed", String(on));
		try {
			localStorage.setItem(WRAP_KEY, on ? "on" : "off");
		} catch {
			// Private browsing may deny storage; the toggle still works per session.
		}
	};
	editorHost.dataset.wrap = isWrapOn() ? "on" : "off";
	toggle.setAttribute("aria-pressed", String(isWrapOn()));
	toggle.addEventListener("click", () => apply(!isWrapOn()));
}

/** Draggable divider between the editor and inspector panels (lg and up). */
function setupSplit(): void {
	const handle = document.querySelector<HTMLElement>("[data-split-handle]");
	if (!handle) return;
	const SPLIT_KEY = "qql-playground.split.v1";
	const MIN = 0.6;
	const MAX = 1.4;
	const DEFAULT_A = 1.08;

	const apply = (a: number): number => {
		const clamped = Math.min(MAX, Math.max(MIN, a));
		workspace.style.setProperty("--split-a", `${clamped.toFixed(3)}fr`);
		workspace.style.setProperty("--split-b", `${(2 - clamped).toFixed(3)}fr`);
		return clamped;
	};

	const saved = Number(localStorage.getItem(SPLIT_KEY));
	if (Number.isFinite(saved) && saved >= MIN && saved <= MAX) apply(saved);

	const fractionFromEvent = (clientX: number): number => {
		const bounds = workspace.getBoundingClientRect();
		const handleWidth = handle.offsetWidth || 8;
		const usable = bounds.width - handleWidth;
		if (usable <= 0) return DEFAULT_A;
		// Columns are [a, handle, b] with a + b = 2, so solve for a in fr units.
		return ((clientX - bounds.left - handleWidth / 2) / usable) * 2;
	};

	handle.addEventListener("pointerdown", (event) => {
		if (!window.matchMedia("(min-width: 64rem)").matches) return;
		event.preventDefault();
		handle.setPointerCapture(event.pointerId);
		handle.dataset.dragging = "true";
		const move = (moveEvent: PointerEvent) => {
			apply(fractionFromEvent(moveEvent.clientX));
		};
		const up = (upEvent: PointerEvent) => {
			const finalSplit = apply(fractionFromEvent(upEvent.clientX));
			handle.dataset.dragging = "false";
			delete handle.dataset.dragging;
			localStorage.setItem(SPLIT_KEY, String(finalSplit));
			editor.requestMeasure();
			handle.removeEventListener("pointermove", move);
			handle.removeEventListener("pointerup", up);
			handle.removeEventListener("pointercancel", up);
		};
		handle.addEventListener("pointermove", move);
		handle.addEventListener("pointerup", up);
		handle.addEventListener("pointercancel", up);
	});

	handle.addEventListener("dblclick", () => {
		apply(DEFAULT_A);
		localStorage.setItem(SPLIT_KEY, String(DEFAULT_A));
		editor.requestMeasure();
	});

	handle.addEventListener("keydown", (event) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const current = Number.parseFloat(
			workspace.style.getPropertyValue("--split-a") || String(DEFAULT_A),
		);
		const next = apply(current + (event.key === "ArrowRight" ? 0.08 : -0.08));
		localStorage.setItem(SPLIT_KEY, String(next));
		editor.requestMeasure();
	});
}

async function start(): Promise<void> {
	applyPlatformKeyHints();
	setupSplit();
	setupWrap();
	setupTabs();
	setupDialogs();
	setupPresets();
	setupShare();
	setupDocsBacklink(pageParams.get("ref"));
	writeSettingsForm();
	writePolicyForm();
	setupExporter();
	const savedTab = loadSession(INSPECTOR_TAB_KEY);
	if (
		[
			"plan",
			"wire",
			"ast",
			"tokens",
			"explain",
			"response",
			"metrics",
		].includes(savedTab ?? "")
	) {
		switchInspectorTab(savedTab as InspectorTab);
	}
	statementSelect.addEventListener("change", () => {
		state.selectedStatement = Number(statementSelect.value);
		renderPlan();
		renderOutputs();
		renderRoutes();
	});
	runButton.addEventListener("click", () => void executeQuery());
	formatButton.addEventListener("click", formatEditor);

	try {
		await initQql();
		configureClient();
		setRuntime("Current qql-rs WASM ready", "ready");
		statusWasm.textContent = "Ready";
		runAnalysis(editor.state.doc.toString());
		setEditorLoading(null);
		void checkEndpoint();
		void loadLivePresets();
	} catch (error) {
		const message = formatError(error);
		setRuntime(`WASM failed: ${message}`, "failed");
		statusWasm.textContent = "Failed to load";
		validationBadge.classList.remove("is-loading");
		validationBadge.classList.add("is-invalid");
		validationBadge.textContent = "WASM unavailable";
		analysisSummary.textContent = message;
		setEditorLoading(
			"QQL WebAssembly could not load. Reload the page to try again.",
		);
		toast(message, "error");
	}
}

window.addEventListener("beforeunload", () => {
	client?.free();
	for (const retired of retiredClients) retired.free();
	retiredClients.clear();
});

void start();
