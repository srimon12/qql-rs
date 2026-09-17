import { toggleComment } from "@codemirror/commands";
import { forceLinting } from "@codemirror/lint";
import type { ExecutionReport } from "qql-wasm-current";
import { listCollections, probeQdrant, QdrantClient } from "./client";
import { createPlaygroundEditor, type PlaygroundEditor } from "./editor";
import { exportCode } from "./export";
import { setupCommandPalette } from "./palette";
import {
	escapeHtml,
	highlightJson,
	pretty,
	renderEmpty,
	renderError,
	renderExecResponse,
	renderExplain,
	renderMetrics,
} from "./render";
import {
	lineAt,
	type StatementSpan,
	scanStatementSpans,
	statementIndexAt,
	statementIndicesInRange,
	statementKeyword,
	statementPreview,
	statementSource,
} from "./statements";
import {
	browserModelInfo,
	DEFAULT_POLICY,
	DEFAULT_RUN_PREFERENCES,
	DEFAULT_SETTINGS,
	type ExportLanguage,
	INSPECTOR_TAB_KEY,
	type InspectorTab,
	MOBILE_VIEW_KEY,
	type PlaygroundAnalysis,
	type PlaygroundMetrics,
	type PlaygroundSettings,
	POLICY_KEY,
	RUN_KEY,
	type RunPreferences,
	type RuntimePolicy,
	SETTINGS_KEY,
	SPLIT_KEY,
	WORKSPACE_KEY,
	WRAP_KEY,
} from "./types";
import {
	analyzeWithPolicy,
	buildFailure,
	byteOffsetToPosition,
	connectionHost,
	currentDiagnostic,
	formatError,
	formatQuery,
	initQql,
	runtimeDiagnostic,
	selectedRoute,
} from "./wasm";

function byId<T extends HTMLElement>(id: string): T {
	const el = document.getElementById(id);
	if (!el) throw new Error(`Missing element #${id}`);
	return el as T;
}

function query<T extends HTMLElement>(
	selector: string,
	root: ParentNode = document,
): T | null {
	return root.querySelector<T>(selector);
}

function queryAll<T extends HTMLElement>(
	selector: string,
	root: ParentNode = document,
): T[] {
	return [...root.querySelectorAll<T>(selector)];
}

// ── State ────────────────────────────────────────────────────────────────────

type StatementStatus = "idle" | "running" | "ok" | "error" | "skipped";
type MobileView = "editor" | "result";

let settings: PlaygroundSettings = loadSettings();
let policy: RuntimePolicy = loadPolicy();
let runPrefs: RunPreferences = loadRunPrefs();
let activeTab: InspectorTab = loadTab();
let activeLanguage: ExportLanguage = "python";
let mobileView: MobileView = loadMobileView();
let wrapEnabled = loadWrap();
let analysis: PlaygroundAnalysis | null = null;
let metrics: PlaygroundMetrics | null = null;
let selectedStmtIndex = 0;
let lastExecutionReport: ExecutionReport | null = null;
let lastExecutionError: ReturnType<typeof buildFailure> | null = null;
let lastExecuteMs: number | null = null;
/** Statement spans of the live document (text-level scan). */
let statementSpans: StatementSpan[] = [];
/** Editor statement indices covered by the last run, in execution order. */
let executedIndices: number[] = [];
const statementStatus = new Map<number, StatementStatus>();
/** Real tensor width reported by the in-browser model, once loaded. */
let browserDims: number | null = null;
/** Editor handle, wired in `start()`. */
let activeEditor: PlaygroundEditor;

const client = new QdrantClient();

// ── Persistence Helpers ──────────────────────────────────────────────────────

function loadSettings(): PlaygroundSettings {
	try {
		const raw = localStorage.getItem(SETTINGS_KEY);
		if (raw) return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
	} catch {
		// use default
	}
	return { ...DEFAULT_SETTINGS };
}

function saveSettings(next: PlaygroundSettings): void {
	settings = next;
	try {
		localStorage.setItem(SETTINGS_KEY, JSON.stringify(next));
	} catch {
		// local storage quota
	}
}

function loadPolicy(): RuntimePolicy {
	try {
		const raw = localStorage.getItem(POLICY_KEY);
		if (raw) return { ...DEFAULT_POLICY, ...JSON.parse(raw) };
	} catch {
		// use default
	}
	return { ...DEFAULT_POLICY };
}

function savePolicy(next: RuntimePolicy): void {
	policy = next;
	try {
		localStorage.setItem(POLICY_KEY, JSON.stringify(next));
	} catch {
		// local storage quota
	}
}

function loadTab(): InspectorTab {
	const val = localStorage.getItem(INSPECTOR_TAB_KEY) as InspectorTab | null;
	return val || "response";
}

function saveTab(tab: InspectorTab): void {
	activeTab = tab;
	try {
		localStorage.setItem(INSPECTOR_TAB_KEY, tab);
	} catch {
		// local storage quota
	}
}

function loadRunPrefs(): RunPreferences {
	try {
		const raw = localStorage.getItem(RUN_KEY);
		if (raw) return { ...DEFAULT_RUN_PREFERENCES, ...JSON.parse(raw) };
	} catch {
		// use default
	}
	return { ...DEFAULT_RUN_PREFERENCES };
}

function saveRunPrefs(next: RunPreferences): void {
	runPrefs = next;
	try {
		localStorage.setItem(RUN_KEY, JSON.stringify(next));
	} catch {
		// local storage quota
	}
}

function loadWrap(): boolean {
	return localStorage.getItem(WRAP_KEY) === "on";
}

function saveWrap(enabled: boolean): void {
	wrapEnabled = enabled;
	try {
		localStorage.setItem(WRAP_KEY, enabled ? "on" : "off");
	} catch {
		// local storage quota
	}
}

function loadMobileView(): MobileView {
	return localStorage.getItem(MOBILE_VIEW_KEY) === "result"
		? "result"
		: "editor";
}

function saveMobileView(view: MobileView): void {
	mobileView = view;
	try {
		localStorage.setItem(MOBILE_VIEW_KEY, view);
	} catch {
		// local storage quota
	}
}

// ── Toast notifications ──────────────────────────────────────────────────────

export function showToast(message: string, isError = false): void {
	const region = query("[data-toast-region]");
	if (!region) return;
	const toast = document.createElement("div");
	toast.className = `toast ${isError ? "toast--error" : ""}`;
	toast.textContent = message;
	region.appendChild(toast);
	window.setTimeout(() => toast.remove(), 3200);
}

// ── UI Updaters ──────────────────────────────────────────────────────────────

/** Result of the selected statement, mapped through the last run's order. */
function resultForStatement(
	index: number,
): ExecutionReport["results"][number] | null {
	if (!lastExecutionReport) return null;
	const position = executedIndices.indexOf(index);
	if (position < 0) return null;
	return lastExecutionReport.results[position] ?? null;
}

function statusDotClass(status: StatementStatus | undefined): string {
	switch (status) {
		case "ok":
			return "bg-[var(--q-ok)]";
		case "error":
			return "bg-[var(--q-bad)]";
		case "running":
			return "bg-[var(--sl-color-accent)] animate-pulse";
		case "skipped":
			return "bg-[var(--sl-color-gray-4)]";
		default:
			return "bg-transparent ring-1 ring-inset ring-[var(--sl-color-gray-5)]";
	}
}

/** Context strip above the response: which statement the panel is showing. */
function updateResponseContext(): void {
	const bar = query("[data-response-context]");
	if (!bar) return;
	const label = query("[data-response-context-label]");
	const meta = query("[data-response-context-meta]");
	const dot = query("[data-response-context-dot]");

	if (!lastExecutionReport && !lastExecutionError) {
		bar.hidden = true;
		return;
	}
	bar.hidden = false;

	const source = activeEditor?.source() ?? "";
	const count = statementSpans.length || analysis?.result.statements_count || 1;
	const span = statementSpans[selectedStmtIndex];
	const keyword = span ? statementKeyword(source, span) : "";
	if (label) {
		label.textContent = span
			? `Statement ${selectedStmtIndex + 1} of ${count} · ${keyword}`
			: `Statement ${selectedStmtIndex + 1} of ${count}`;
	}

	const status = statementStatus.get(selectedStmtIndex);
	if (dot) {
		dot.className = `size-1.5 shrink-0 rounded-full ${statusDotClass(status)}`;
	}

	const res = resultForStatement(selectedStmtIndex);
	if (meta) {
		if (lastExecutionError && status === "error") {
			meta.textContent = lastExecutionError.code ?? "Run failed";
		} else if (!res) {
			meta.textContent = count > 1 ? "Not run · ⌘↵ runs this statement" : "";
		} else if (!res.ok) {
			meta.textContent = "Failed · see the result card";
		} else {
			const timing =
				executedIndices.length === 1 && lastExecuteMs !== null
					? ` · ${lastExecuteMs.toFixed(1)} ms`
					: "";
			meta.textContent = `${res.operation}${timing}`;
		}
	}
}

function updateStatusBar(): void {
	const validationBadge = query("[data-validation-badge]");
	const summary = query("[data-analysis-summary]");
	const stmtCount = query("[data-stmt-count]");
	const policyDot = query("[data-policy-dot]");
	const policyChip = query("[data-policy-chip]");
	const policyChipValue = query("[data-policy-chip-value]");
	const embedChip = query("[data-status-embed]");
	const embedDimChip = query("[data-embed-dim-chip]");

	if (embedChip) {
		if (settings.embedProvider === "browser") {
			embedChip.textContent = `${browserModelInfo(settings.embedBrowserModel).label} (browser)`;
		} else if (settings.embedProvider === "http") {
			embedChip.textContent = `${settings.embedModel || "HTTP"}`;
		} else embedChip.textContent = "None (raw vectors)";
	}
	if (embedDimChip) {
		embedDimChip.hidden = settings.embedProvider === "none";
		if (settings.embedProvider === "browser") {
			const dims =
				browserDims ?? browserModelInfo(settings.embedBrowserModel).dims;
			embedDimChip.textContent = dims ? `${dims}d` : "auto";
		} else {
			embedDimChip.textContent = `${settings.embedDim}d`;
		}
	}

	if (policyDot) {
		policyDot.classList.toggle("is-active", policy.enabled);
	}
	if (policyChip && policyChipValue) {
		policyChip.hidden = !policy.enabled;
		policyChipValue.textContent = `${policy.field} ${policy.op} ${policy.value}`;
	}

	if (!analysis) return;

	if (stmtCount) {
		const count = statementSpans.length || analysis.result.statements_count;
		stmtCount.textContent = `${count} ${count === 1 ? "stmt" : "stmts"}`;
	}

	if (validationBadge) {
		validationBadge.classList.remove("is-valid", "is-invalid");
		if (analysis.result.valid && !analysis.policyError) {
			validationBadge.classList.add("is-valid");
			validationBadge.innerHTML = `<span class="size-1.5 rounded-full bg-[var(--q-ok)]"></span> Valid QQL`;
		} else {
			validationBadge.classList.add("is-invalid");
			validationBadge.innerHTML = `<span class="size-1.5 rounded-full bg-[var(--q-bad)]"></span> Syntax error`;
		}
	}

	if (summary) {
		if (analysis.policyError) {
			summary.textContent = analysis.policyError;
		} else if (!analysis.result.valid) {
			const err = analysis.result.error;
			summary.textContent = err
				? `${err.code}: ${err.message}`
				: "Syntax error in statement";
		} else {
			const route = selectedRoute(analysis, selectedStmtIndex);
			const method = route?.method ?? "POST";
			const path = route?.path ?? "";
			summary.textContent = `Ready · ${method} ${path}`;
		}
	}
}

function updateInspectorPanels(): void {
	// 1. Update active tab
	queryAll("[data-inspector-tab]").forEach((btn) => {
		const tab = btn.dataset.inspectorTab as InspectorTab;
		const isSelected = tab === activeTab;
		btn.setAttribute("aria-selected", isSelected ? "true" : "false");
		btn.tabIndex = isSelected ? 0 : -1;
	});

	queryAll("[data-inspector-panel]").forEach((panel) => {
		const tab = panel.dataset.inspectorPanel as InspectorTab;
		panel.hidden = tab !== activeTab;
	});

	if (!analysis) return;

	// 2. Update statement navigator in the inspector header
	const stmtSelect = query<HTMLSelectElement>("[data-statement-select]");
	const stmtWrap = query("[data-statement-wrap]");
	const count = statementSpans.length || analysis.result.statements_count;
	if (stmtSelect && stmtWrap) {
		stmtWrap.hidden = count <= 1;
		if (count > 1) {
			const source = activeEditor?.source() ?? "";
			stmtSelect.innerHTML = statementSpans
				.map((span, index) => {
					const status = statementStatus.get(index);
					const mark =
						status === "ok"
							? " ✓"
							: status === "error"
								? " ✕"
								: status === "skipped"
									? " –"
									: "";
					const kind =
						source.length > 0 ? statementKeyword(source, span) : "QQL";
					return `<option value="${index}" ${index === selectedStmtIndex ? "selected" : ""}>${index + 1} · ${escapeHtml(kind)}${mark}</option>`;
				})
				.join("");
		}
	}

	const route = selectedRoute(analysis, selectedStmtIndex);
	const routeLine = query("[data-route-line]");
	if (routeLine) {
		// Redundant with the Plan tab once per-statement navigation appears.
		routeLine.hidden = count > 1;
		routeLine.textContent = route ? `${route.method} ${route.path}` : "";
	}

	// 3. Update Plan Panel
	const emptyPlan = query("[data-empty-plan]");
	const planCard = query("[data-plan-card]");
	if (emptyPlan && planCard) {
		const hasPlan = Boolean(analysis.result.valid && route);
		emptyPlan.hidden = hasPlan;
		planCard.hidden = !hasPlan;
		if (hasPlan && route) {
			const methodEl = query("[data-route-method]");
			const pathEl = query("[data-route-path]");
			const typeEl = query("[data-route-type]");
			const policyEl = query("[data-route-policy]");
			const countEl = query("[data-routes-count]");
			const listEl = query("[data-routes-list]");

			if (methodEl) methodEl.textContent = route.method;
			if (pathEl) pathEl.textContent = route.path;
			if (typeEl) typeEl.textContent = "Query / Mutation";
			if (policyEl)
				policyEl.textContent = policy.enabled
					? `${policy.field} ${policy.op} ${policy.value}`
					: "Source only";
			if (countEl) countEl.textContent = String(count);

			if (listEl) {
				listEl.innerHTML = analysis.effectiveRoutes
					.map((r, idx) => {
						const status = statementStatus.get(idx);
						const mark =
							status === "ok"
								? `<span class="size-1.5 rounded-full bg-[var(--q-ok)]" title="Ran successfully"></span>`
								: status === "error"
									? `<span class="size-1.5 rounded-full bg-[var(--q-bad)]" title="Failed"></span>`
									: status === "running"
										? `<span class="size-1.5 animate-pulse rounded-full bg-[var(--sl-color-accent)]" title="Running"></span>`
										: "";
						return `
					<button type="button" class="route-row" aria-current="${idx === selectedStmtIndex ? "true" : "false"}" data-select-stmt="${idx}">
						<span class="route-row__index">${idx + 1}</span>
						<span class="route-row__type">${escapeHtml(r.path)}</span>
						<span class="route-row__method">${escapeHtml(r.method)}</span>
						${mark ? `<span class="col-start-3 row-start-1 justify-self-end">${mark}</span>` : ""}
					</button>
				`;
					})
					.join("");
			}
		}
	}

	// 4. Update Wire / AST / Tokens / Explain / Metrics pre tabs
	const wireOutput = query(`[data-output="wire"]`);
	if (wireOutput) {
		wireOutput.innerHTML = route
			? highlightJson(pretty(route.payload))
			: "No compiled route available.";
	}

	const astOutput = query(`[data-output="ast"]`);
	if (astOutput) {
		const currentAst = analysis.effectiveAst?.[selectedStmtIndex] ?? null;
		astOutput.innerHTML = currentAst
			? highlightJson(pretty(currentAst))
			: "No AST available.";
	}

	const tokensOutput = query(`[data-output="tokens"]`);
	if (tokensOutput) {
		tokensOutput.innerHTML = highlightJson(pretty(analysis.result.tokens));
	}

	const explainRich = query("[data-explain-rich]");
	const explainOutput = query(`[data-output="explain"]`);
	const explainText =
		analysis.result.explain ||
		(analysis.policyError ? analysis.policyError : null);
	if (explainRich) {
		renderExplain(explainRich, explainText);
	}
	if (explainOutput) {
		explainOutput.textContent = explainText || "No explanation available.";
	}

	const metricsCards = query("[data-metrics-cards]");
	const metricsOutput = query(`[data-output="metrics"]`);
	const currentRes = resultForStatement(selectedStmtIndex);
	const rawTelemetry =
		(lastExecutionReport as unknown as { telemetry?: Record<string, unknown> })
			?.telemetry ??
		(currentRes as unknown as { telemetry?: Record<string, unknown> })
			?.telemetry ??
		null;

	if (metricsCards) {
		renderMetrics(metricsCards, metrics, rawTelemetry);
	}
	if (metricsOutput && metrics) {
		metricsOutput.innerHTML = highlightJson(
			pretty({
				...metrics,
				telemetry: rawTelemetry,
				reportSummary: lastExecutionReport
					? {
							ok: lastExecutionReport.ok,
							succeeded: lastExecutionReport.succeeded,
							failed: lastExecutionReport.failed,
							resultsCount: lastExecutionReport.results?.length ?? 0,
						}
					: null,
			}),
		);
	}

	// 5. Update Response Tab
	const responseRich = query("[data-response-rich]");
	const responsePre = query(`[data-output="response"]`);

	if (responseRich) {
		const hitBadge = query("[data-hit-count-badge]");
		if (lastExecutionError) {
			renderError(responseRich, lastExecutionError);
			if (hitBadge) hitBadge.hidden = true;
			if (responsePre) {
				responsePre.textContent = lastExecutionError.raw;
				responsePre.hidden = true;
			}
		} else if (lastExecutionReport) {
			const report = lastExecutionReport;
			if (responsePre) {
				responsePre.innerHTML = highlightJson(pretty(report));
				responsePre.hidden = true;
			}

			const res = resultForStatement(selectedStmtIndex);
			if (!res) {
				renderEmpty(
					responseRich,
					`Statement ${selectedStmtIndex + 1} has not run in this session. Press ⌘↵ (Ctrl+Enter) to run it, or choose Run all statements.`,
				);
				if (hitBadge) hitBadge.hidden = true;
			} else if (res.ok === false) {
				// Batched runs report per-statement failures in-band; render the
				// structured card rather than a raw message.
				renderError(
					responseRich,
					buildFailure(res.message ?? "Statement failed", settings.qdrantUrl),
				);
				if (hitBadge) hitBadge.hidden = true;
			} else {
				const badgeCount = renderExecResponse(responseRich, res, lastExecuteMs);
				if (hitBadge) {
					if (badgeCount == null) {
						hitBadge.hidden = true;
					} else {
						hitBadge.textContent = String(badgeCount);
						hitBadge.hidden = false;
					}
				}
			}
		} else {
			renderEmpty(responseRich);
			if (hitBadge) hitBadge.hidden = true;
			if (responsePre) {
				responsePre.textContent =
					"Run a query against your reachable Qdrant to see live hits.";
				responsePre.hidden = true;
			}
		}
	}

	updateResponseContext();
}

// ── Main Start Routine ───────────────────────────────────────────────────────

export async function start(): Promise<void> {
	const workspace = query<HTMLElement>("[data-workspace]");
	const editorHost = byId("qql-editor");
	const runBtn = query<HTMLButtonElement>("[data-run]");
	const formatBtn = query<HTMLButtonElement>("[data-format]");
	const activeFixture = query("[data-active-fixture]");
	const editorLoading = query("[data-editor-loading]");

	// 1. Initial document: ?q= query parameter takes priority (e.g. from Quickstart or Docs "Try in playground")
	const pageParams = new URLSearchParams(window.location.search);
	const urlQuery = pageParams.get("q");
	const urlRef = pageParams.get("ref") || pageParams.get("from");
	const savedSource = localStorage.getItem(WORKSPACE_KEY);
	const defaultQuery =
		workspace?.dataset.defaultQuery ||
		"QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5;";
	const initialDoc = urlQuery ?? savedSource ?? defaultQuery;

	if (urlQuery && activeFixture) {
		activeFixture.textContent = urlRef?.includes("quickstart")
			? "Quickstart"
			: urlRef
				? "From docs"
				: "Shared query";
	}

	const docsBacklink = query<HTMLAnchorElement>("[data-docs-backlink]");
	if (docsBacklink && urlRef) {
		docsBacklink.href = urlRef;
		docsBacklink.hidden = false;
		docsBacklink.textContent = urlRef.includes("quickstart")
			? "Back to Quickstart →"
			: "Back to docs →";
	}

	// 2. Editor setup
	const runAnalysis = (source: string) => {
		statementSpans = scanStatementSpans(source);
		const out = analyzeWithPolicy(
			source,
			policy,
			settings.embedProvider,
			lastExecuteMs,
		);
		analysis = out.analysis;
		metrics = out.metrics;
		updateStatusBar();
		updateInspectorPanels();
		updateStatementRail();
	};

	let debounceTimer: number | null = null;
	const queueAnalysis = (source: string) => {
		statementSpans = scanStatementSpans(source);
		syncStatementState();
		updateStatementRail();
		if (debounceTimer) window.clearTimeout(debounceTimer);
		debounceTimer = window.setTimeout(() => {
			runAnalysis(source);
			try {
				localStorage.setItem(WORKSPACE_KEY, source);
			} catch {
				// storage error
			}
		}, 80);
	};

	/** Keep selection/statuses inside the live statement list. */
	const syncStatementState = () => {
		const count = statementSpans.length;
		for (const index of [...statementStatus.keys()]) {
			if (index >= count) statementStatus.delete(index);
		}
		selectedStmtIndex = Math.min(
			Math.max(0, selectedStmtIndex),
			Math.max(0, count - 1),
		);
	};

	const selectStatement = (
		index: number,
		options: { reveal?: boolean } = {},
	): void => {
		const count = Math.max(1, statementSpans.length);
		selectedStmtIndex = Math.min(Math.max(0, index), count - 1);
		updateStatementRail();
		updateInspectorPanels();
		updateRunMenuHints();
		if (options.reveal && statementSpans[selectedStmtIndex]) {
			activeEditor.revealStatement(selectedStmtIndex);
		}
	};

	function updateStatementRail(): void {
		const rail = query("[data-statement-rail]");
		const posChip = query("[data-statement-pos]");
		const source = activeEditor?.source() ?? "";
		if (posChip) {
			posChip.hidden = statementSpans.length <= 1;
			posChip.textContent = `Stmt ${selectedStmtIndex + 1}/${statementSpans.length || 1}`;
		}
		if (!rail) return;
		rail.hidden = statementSpans.length <= 1;
		if (statementSpans.length <= 1) {
			rail.innerHTML = "";
			return;
		}
		rail.innerHTML = statementSpans
			.map((span, index) => {
				const kind = statementKeyword(source, span);
				const status = statementStatus.get(index);
				const active = index === selectedStmtIndex;
				return `<button type="button" class="app-chip h-7 shrink-0 pl-2" data-stmt-chip="${index}" aria-current="${active}" title="${escapeHtml(statementPreview(source, span, 160))}">
					<span class="font-mono text-[0.62rem] font-bold tabular-nums text-[var(--sl-color-gray-3)]">${index + 1}</span>
					<span class="font-mono text-[0.68rem] font-semibold">${escapeHtml(kind)}</span>
					<span class="size-1.5 shrink-0 rounded-full ${statusDotClass(status)}"></span>
				</button>`;
			})
			.join("");
	}

	const updateCursorStatus = (): void => {
		const el = query("[data-cursor-pos]");
		if (!el || !activeEditor) return;
		const pos = activeEditor.view.state.selection.main.head;
		const doc = activeEditor.view.state.doc;
		const line = doc.lineAt(Math.min(pos, doc.length));
		el.textContent = `Ln ${line.number}, Col ${pos - line.from + 1}`;
	};

	const followCursor = (pos: number): void => {
		updateCursorStatus();
		const index = statementIndexAt(statementSpans, pos);
		if (index >= 0 && index !== selectedStmtIndex) {
			selectedStmtIndex = index;
			updateStatementRail();
			updateInspectorPanels();
			updateRunMenuHints();
		}
	};

	activeEditor = createPlaygroundEditor({
		parent: editorHost,
		doc: initialDoc,
		diagnostics: () => [
			...currentDiagnostic(analysis),
			...runtimeDiagnostic(lastExecutionError, activeEditor?.source() ?? ""),
		],
		onDocChange: (source) => {
			if (activeFixture) activeFixture.textContent = "Custom query";
			queueAnalysis(source);
		},
		onCursor: (pos) => followCursor(pos),
		onRun: (shortcut) => {
			void (shortcut === "all" ? runAll() : runSmart());
		},
		onFormat: () => formatDocument(),
		onSelectStatement: (index) => selectStatement(index, { reveal: true }),
	});

	// 3. Execution logic — selection, then cursor statement, then exact line.
	type RunMode = "smart" | "all" | "statement" | "from";
	interface RunTarget {
		text: string;
		/** Editor statement indices this run covers, in execution order. */
		statements: number[];
		label: string;
	}

	const isMac = /Mac|iPhone|iPad|iPod/.test(
		`${navigator.platform} ${navigator.userAgent}`,
	);
	queryAll("[data-kbd], [data-kbd-short]").forEach((kbd) => {
		kbd.textContent = isMac ? "⌘↵" : "Ctrl+↵";
	});
	queryAll("[data-kbd-all]").forEach((kbd) => {
		kbd.textContent = isMac ? "⇧⌘↵" : "Shift+Ctrl+↵";
	});
	queryAll("[data-open-palette] kbd").forEach((kbd) => {
		kbd.textContent = isMac ? "⌘K" : "Ctrl+K";
	});

	const setBusy = (busy: boolean): void => {
		if (!runBtn) return;
		runBtn.disabled = busy || !client.ready;
		runBtn.toggleAttribute("aria-busy", busy);
		const runLabel = query("[data-run-label]");
		if (runLabel) runLabel.textContent = busy ? "Running…" : "Run";
	};

	const resolveRunTarget = (mode: RunMode): RunTarget | null => {
		const source = activeEditor.source();
		const spans = statementSpans;
		if (source.trim().length === 0) return null;

		if (mode === "all") {
			const label =
				spans.length === 1 ? "1 statement" : `${spans.length} statements`;
			return {
				text: source,
				statements: spans.map((_, index) => index),
				label,
			};
		}

		if (mode === "smart") {
			const selection = activeEditor.selectionText();
			if (selection) {
				const range = activeEditor.view.state.selection.main;
				return {
					text: selection,
					statements: statementIndicesInRange(spans, range.from, range.to),
					label: "selection",
				};
			}
		}

		const index = activeEditor.cursorStatement();
		if (index < 0 || index >= spans.length) {
			// Comment-only or broken document: run the exact line under the cursor.
			const line = lineAt(source, activeEditor.view.state.selection.main.head);
			const text = line.text.trim();
			return text ? { text, statements: [], label: "current line" } : null;
		}
		if (mode === "from") {
			const text = source.slice(spans[index].start).trim();
			if (!text) return null;
			return {
				text,
				statements: spans.map((_, i) => i).slice(index),
				label: `statements ${index + 1}–${spans.length}`,
			};
		}
		const text = statementSource(source, spans[index]);
		if (!text) return null;
		return { text, statements: [index], label: `statement ${index + 1}` };
	};

	const switchTab = (tab: InspectorTab): void => {
		activeTab = tab;
		saveTab(tab);
	};

	async function runTarget(target: RunTarget): Promise<void> {
		if (policy.enabled && target.statements.length > 1) {
			showToast(
				"Tenant policy applies to one statement at a time — run a single statement or disable the policy",
				true,
			);
			return;
		}
		for (const index of target.statements)
			statementStatus.set(index, "running");
		updateStatementRail();
		setBusy(true);
		const started = performance.now();
		try {
			lastExecutionError = null;
			lastExecutionReport = await client.execute(target.text, policy, {
				onError: runPrefs.stopOnError ? "stop" : "continue",
			});
			lastExecuteMs = performance.now() - started;
			const results = lastExecutionReport.results;
			executedIndices = target.statements.slice(0, results.length);
			executedIndices.forEach((statementIndex, position) => {
				statementStatus.set(
					statementIndex,
					results[position]?.ok ? "ok" : "error",
				);
			});
			for (const index of target.statements.slice(results.length)) {
				statementStatus.set(index, "skipped");
			}
			const firstFailure = executedIndices.find(
				(index) => statementStatus.get(index) === "error",
			);
			if (firstFailure !== undefined) selectedStmtIndex = firstFailure;
			else if (executedIndices.length > 0)
				selectedStmtIndex = executedIndices[0];
			switchTab("response");
			const failed = lastExecutionReport.failed ?? 0;
			if (failed === 0) showToast(`Ran ${target.label}`);
			else showToast(`${failed} of ${results.length} statements failed`, true);
			forceLinting(activeEditor.view);
		} catch (error) {
			lastExecutionReport = null;
			lastExecuteMs = performance.now() - started;
			lastExecutionError = buildFailure(error, settings.qdrantUrl);
			executedIndices = [];
			// A stopped multi-statement run reports one error; its span points at
			// the failing statement. Script parse errors surface through the
			// analyzer instead, so fall back to its first diagnostic span; with
			// neither, only the first statement is claimed.
			const failureSpan =
				lastExecutionError.span ??
				(() => {
					const first = analysis?.result.errors?.[0] ?? analysis?.result.error;
					return first && first.start != null && first.end != null
						? { start: first.start, end: first.end }
						: null;
				})();
			const failingIndex = failureSpan
				? statementIndexAt(
						statementSpans,
						byteOffsetToPosition(target.text, failureSpan.start),
					)
				: -1;
			if (target.statements.length <= 1) {
				for (const index of target.statements)
					statementStatus.set(index, "error");
			} else {
				const stopAt =
					failingIndex >= 0 ? target.statements.indexOf(failingIndex) : 0;
				target.statements.forEach((index, position) => {
					statementStatus.set(
						index,
						position < stopAt
							? "ok"
							: position === stopAt
								? "error"
								: "skipped",
					);
				});
			}
			switchTab("response");
			showToast(lastExecutionError.message, true);
		} finally {
			setBusy(false);
			runAnalysis(activeEditor.source());
			forceLinting(activeEditor.view);
		}
	}

	async function runSmart(): Promise<void> {
		const target = resolveRunTarget("smart");
		if (target) await runTarget(target);
	}

	async function runAll(): Promise<void> {
		const target = resolveRunTarget("all");
		if (target) await runTarget(target);
	}

	if (runBtn) {
		runBtn.addEventListener("click", () => void runSmart());
	}

	// Run menu: extra targets + batch error policy.
	const runMenu = query("[data-run-menu]");
	const runMenuToggle = query<HTMLButtonElement>("[data-run-menu-toggle]");
	const setRunMenu = (open: boolean): void => {
		if (!runMenu || !runMenuToggle) return;
		runMenu.hidden = !open;
		runMenuToggle.setAttribute("aria-expanded", String(open));
	};
	const updateRunMenuHints = (): void => {
		const hint = query("[data-run-statement-hint]");
		if (hint) hint.textContent = `Stmt ${selectedStmtIndex + 1}`;
	};
	runMenuToggle?.addEventListener("click", (event) => {
		event.stopPropagation();
		setRunMenu(runMenu ? runMenu.hidden !== false : true);
		updateRunMenuHints();
	});
	queryAll("[data-run-mode]").forEach((btn) => {
		btn.addEventListener("click", () => {
			const mode = btn.dataset.runMode as RunMode;
			setRunMenu(false);
			const target = resolveRunTarget(mode);
			if (target) void runTarget(target);
		});
	});
	const stopOnErrorInput = query<HTMLInputElement>("[data-run-stop-on-error]");
	if (stopOnErrorInput) {
		stopOnErrorInput.checked = runPrefs.stopOnError;
		stopOnErrorInput.addEventListener("change", () => {
			saveRunPrefs({ stopOnError: stopOnErrorInput.checked });
			showToast(
				stopOnErrorInput.checked
					? "Multi-statement runs stop on the first error"
					: "Multi-statement runs continue past errors",
			);
		});
	}

	// 4. Format logic
	const formatDocument = (): void => {
		const source = activeEditor.source();
		try {
			const formatted = formatQuery(source);
			if (formatted && formatted !== source) {
				activeEditor.setSource(formatted);
				showToast("Query formatted");
			}
		} catch (error) {
			const failure = buildFailure(error, settings.qdrantUrl);
			showToast(
				failure.code ? `${failure.code}: ${failure.message}` : failure.message,
				true,
			);
		}
	};
	if (formatBtn) {
		formatBtn.addEventListener("click", formatDocument);
	}

	// 5. Wrap toggle
	const wrapBtn = query<HTMLButtonElement>("[data-wrap-toggle]");
	const applyWrap = (): void => {
		editorHost.dataset.wrap = wrapEnabled ? "on" : "off";
		wrapBtn?.setAttribute("aria-pressed", String(wrapEnabled));
		activeEditor.requestMeasure();
	};
	const toggleWrap = (): void => {
		saveWrap(!wrapEnabled);
		applyWrap();
	};
	if (wrapBtn) {
		wrapBtn.addEventListener("click", toggleWrap);
	}
	applyWrap();
	applyMobileView();

	// 6. Split resizing
	const splitHandle = query("[data-split-handle]");
	if (splitHandle && workspace) {
		const applySplit = (a: number) => {
			const clamped = Math.min(1.4, Math.max(0.6, a));
			workspace.style.setProperty("--split-a", `${clamped.toFixed(3)}fr`);
			workspace.style.setProperty("--split-b", `${(2 - clamped).toFixed(3)}fr`);
			return clamped;
		};

		const saved = Number(localStorage.getItem(SPLIT_KEY));
		if (Number.isFinite(saved) && saved >= 0.6 && saved <= 1.4)
			applySplit(saved);

		splitHandle.addEventListener("pointerdown", (e) => {
			if (!window.matchMedia("(min-width: 64rem)").matches) return;
			e.preventDefault();
			splitHandle.setPointerCapture(e.pointerId);
			const bounds = workspace.getBoundingClientRect();
			const onMove = (moveEvent: PointerEvent) => {
				const frac =
					((moveEvent.clientX - bounds.left - 4) / (bounds.width - 8)) * 2;
				applySplit(frac);
			};
			const onUp = (upEvent: PointerEvent) => {
				const final = applySplit(
					((upEvent.clientX - bounds.left - 4) / (bounds.width - 8)) * 2,
				);
				localStorage.setItem(SPLIT_KEY, String(final));
				activeEditor.requestMeasure();
				splitHandle.removeEventListener("pointermove", onMove);
				splitHandle.removeEventListener("pointerup", onUp);
			};
			splitHandle.addEventListener("pointermove", onMove);
			splitHandle.addEventListener("pointerup", onUp);
		});

		splitHandle.addEventListener("dblclick", () => {
			applySplit(1.08);
			localStorage.setItem(SPLIT_KEY, "1.08");
			activeEditor.requestMeasure();
		});
	}

	// 7. Inspector tab switching
	queryAll("[data-inspector-tab]").forEach((tabBtn) => {
		tabBtn.addEventListener("click", () => {
			const tab = tabBtn.dataset.inspectorTab as InspectorTab;
			if (tab) {
				saveTab(tab);
				updateInspectorPanels();
			}
		});
	});

	// Statement navigation (inspector header + rail chips + plan rows)
	const stmtSelect = query<HTMLSelectElement>("[data-statement-select]");
	stmtSelect?.addEventListener("change", () => {
		selectStatement(Number(stmtSelect.value) || 0, { reveal: true });
	});
	query("[data-stmt-prev]")?.addEventListener("click", () => {
		selectStatement(selectedStmtIndex - 1, { reveal: true });
	});
	query("[data-stmt-next]")?.addEventListener("click", () => {
		selectStatement(selectedStmtIndex + 1, { reveal: true });
	});

	// Mobile view switch: the split stacks below lg, so let phones choose a pane.
	function applyMobileView(): void {
		if (!workspace) return;
		workspace.dataset.mobileView = mobileView;
		queryAll<HTMLButtonElement>("[data-mobile-view]").forEach((btn) => {
			btn.setAttribute(
				"aria-pressed",
				String(btn.dataset.mobileView === mobileView),
			);
		});
	}
	queryAll<HTMLButtonElement>("[data-mobile-view]").forEach((btn) => {
		btn.addEventListener("click", () => {
			saveMobileView(btn.dataset.mobileView === "result" ? "result" : "editor");
			applyMobileView();
			activeEditor.requestMeasure();
		});
	});

	// Copy and interaction click delegates
	document.addEventListener("click", (e) => {
		const target = e.target as HTMLElement;

		// Statement rail chip
		const chip = target.closest<HTMLButtonElement>("[data-stmt-chip]");
		if (chip?.dataset.stmtChip) {
			selectStatement(Number(chip.dataset.stmtChip), { reveal: true });
			return;
		}

		// Compiled route row
		const routeRow = target.closest<HTMLButtonElement>("[data-select-stmt]");
		if (routeRow?.dataset.selectStmt) {
			selectStatement(Number(routeRow.dataset.selectStmt), { reveal: true });
			return;
		}

		// Copy JSON button
		const copyJsonBtn = target.closest<HTMLButtonElement>("[data-copy-json]");
		if (copyJsonBtn?.dataset.copyJson) {
			void navigator.clipboard.writeText(copyJsonBtn.dataset.copyJson);
			showToast("Copied JSON to clipboard");
			return;
		}

		// Copy Text button (e.g. ID)
		const copyTextBtn = target.closest<HTMLButtonElement>("[data-copy-text]");
		if (copyTextBtn?.dataset.copyText) {
			void navigator.clipboard.writeText(copyTextBtn.dataset.copyText);
			showToast("Copied ID to clipboard");
			return;
		}

		// Load Query button (e.g. collection queries)
		const loadQueryBtn = target.closest<HTMLButtonElement>("[data-load-query]");
		if (loadQueryBtn?.dataset.loadQuery) {
			const q = loadQueryBtn.dataset.loadQuery;
			activeEditor.setSource(q);
			showToast("Loaded query into editor");
			return;
		}

		// Copy Tab button (wire, ast, tokens)
		const copyTabBtn = target.closest<HTMLButtonElement>("[data-copy-tab]");
		if (copyTabBtn?.dataset.copyTab) {
			const tabId = copyTabBtn.dataset.copyTab;
			const pre = query(`[data-output="${tabId}"]`);
			if (pre?.textContent) {
				void navigator.clipboard.writeText(pre.textContent);
				showToast(`Copied ${tabId} to clipboard`);
			}
			return;
		}
	});

	// 8. Dialog handling
	const setupDialog = (dialogId: string, openSelectors: string[]) => {
		const dialog = byId<HTMLDialogElement>(dialogId);
		openSelectors.forEach((sel) => {
			queryAll(sel).forEach((trigger) => {
				trigger.addEventListener("click", () => dialog.showModal());
			});
		});
		queryAll("[data-close-dialog]", dialog).forEach((close) => {
			close.addEventListener("click", () => dialog.close());
		});
		dialog.addEventListener("click", (e) => {
			if (e.target === dialog) dialog.close();
		});
	};

	setupDialog("preset-dialog", ["[data-open-presets]"]);
	setupDialog("settings-dialog", ["[data-open-settings]"]);
	setupDialog("policy-dialog", ["[data-open-policy]", "[data-policy-chip]"]);
	setupDialog("export-dialog", ["[data-open-export]"]);

	// Presets search & filtering
	const presetDialog = byId<HTMLDialogElement>("preset-dialog");
	const presetSearch = query<HTMLInputElement>(
		"[data-preset-search]",
		presetDialog,
	);
	const presetCategoryBtns = queryAll<HTMLButtonElement>(
		"[data-preset-category]",
		presetDialog,
	);
	let activeCategory = "all";

	const filterPresets = () => {
		const term = (presetSearch?.value || "").toLowerCase().trim();
		queryAll<HTMLElement>("[data-preset]", presetDialog).forEach((item) => {
			const cat = item.dataset.presetCategoryValue || "";
			const text = (item.textContent || "").toLowerCase();
			const catMatch = activeCategory === "all" || cat === activeCategory;
			const termMatch = !term || text.includes(term);
			item.hidden = !(catMatch && termMatch);
		});
	};

	if (presetSearch) presetSearch.addEventListener("input", filterPresets);
	presetCategoryBtns.forEach((btn) => {
		btn.addEventListener("click", () => {
			presetCategoryBtns.forEach((b) => {
				b.setAttribute("aria-pressed", "false");
			});
			btn.setAttribute("aria-pressed", "true");
			activeCategory = btn.dataset.presetCategory || "all";
			filterPresets();
		});
	});

	// Presets click to load
	queryAll<HTMLElement>("[data-preset-query]", presetDialog).forEach((item) => {
		item.addEventListener("click", () => {
			const q = item.dataset.presetQuery;
			const label = item.dataset.presetLabel || "Example";
			if (q) {
				activeEditor.setSource(q);
				if (activeFixture) activeFixture.textContent = label;
				presetDialog.close();
				showToast(`Loaded example: ${label}`);
			}
		});
	});

	// Settings Form
	const settingsForm = query<HTMLFormElement>("[data-settings-form]");
	if (settingsForm) {
		// Populate inputs
		const urlInput = settingsForm.elements.namedItem(
			"qdrantUrl",
		) as HTMLInputElement | null;
		const keyInput = settingsForm.elements.namedItem(
			"qdrantKey",
		) as HTMLInputElement | null;
		const providerSelect = settingsForm.elements.namedItem(
			"embedProvider",
		) as HTMLSelectElement | null;
		const embedUrlInput = settingsForm.elements.namedItem(
			"embedUrl",
		) as HTMLInputElement | null;
		const embedModelInput = settingsForm.elements.namedItem(
			"embedModel",
		) as HTMLInputElement | null;
		const embedDimInput = settingsForm.elements.namedItem(
			"embedDim",
		) as HTMLInputElement | null;
		const embedKeyInput = settingsForm.elements.namedItem(
			"embedKey",
		) as HTMLInputElement | null;
		const browserModelInput = settingsForm.elements.namedItem(
			"embedBrowserModel",
		) as HTMLInputElement | null;
		const browserDeviceSelect = settingsForm.elements.namedItem(
			"embedBrowserDevice",
		) as HTMLSelectElement | null;
		const browserFields = query("[data-browser-embed-fields]", settingsForm);
		const httpFields = query("[data-http-embed-fields]", settingsForm);
		const browserDimsEl = query("[data-browser-model-dims]", settingsForm);

		if (urlInput) urlInput.value = settings.qdrantUrl;
		if (keyInput) keyInput.value = settings.qdrantKey;
		if (providerSelect) providerSelect.value = settings.embedProvider;
		if (embedUrlInput) embedUrlInput.value = settings.embedUrl;
		if (embedModelInput) embedModelInput.value = settings.embedModel;
		if (embedDimInput) embedDimInput.value = String(settings.embedDim);
		if (embedKeyInput) embedKeyInput.value = settings.embedKey;
		if (browserModelInput) browserModelInput.value = settings.embedBrowserModel;
		if (browserDeviceSelect)
			browserDeviceSelect.value = settings.embedBrowserDevice;

		const updateBrowserDims = (): void => {
			if (!browserDimsEl) return;
			const model = (browserModelInput?.value || "").trim();
			const known = model ? browserModelInfo(model).dims : null;
			const dims = browserDims ?? known;
			browserDimsEl.textContent = dims
				? `${dims} dimensions reported`
				: "Reported after the first embedding";
		};

		const updateProviderPanels = (): void => {
			const provider = providerSelect?.value ?? settings.embedProvider;
			if (browserFields) browserFields.hidden = provider !== "browser";
			if (httpFields) httpFields.hidden = provider !== "http";
			updateBrowserDims();
		};
		providerSelect?.addEventListener("change", updateProviderPanels);
		browserModelInput?.addEventListener("input", updateBrowserDims);
		updateProviderPanels();

		settingsForm.addEventListener("submit", (e) => {
			e.preventDefault();
			const formData = new FormData(settingsForm);
			const provider =
				(formData.get(
					"embedProvider",
				) as PlaygroundSettings["embedProvider"]) || "browser";
			const browserModel = String(
				formData.get("embedBrowserModel") || "",
			).trim();
			if (provider === "browser" && !browserModel) {
				showToast(
					"Enter an in-browser model id, e.g. Xenova/bge-small-en-v1.5",
					true,
				);
				browserModelInput?.focus();
				return;
			}
			const next: PlaygroundSettings = {
				qdrantUrl: String(
					formData.get("qdrantUrl") || DEFAULT_SETTINGS.qdrantUrl,
				).trim(),
				qdrantKey: String(formData.get("qdrantKey") || "").trim(),
				embedProvider: provider,
				embedUrl: String(
					formData.get("embedUrl") || DEFAULT_SETTINGS.embedUrl,
				).trim(),
				embedModel: String(
					formData.get("embedModel") || DEFAULT_SETTINGS.embedModel,
				).trim(),
				embedDim: Number(formData.get("embedDim")) || DEFAULT_SETTINGS.embedDim,
				embedKey: String(formData.get("embedKey") || "").trim(),
				embedBrowserModel: browserModel || DEFAULT_SETTINGS.embedBrowserModel,
				embedBrowserDevice:
					(formData.get(
						"embedBrowserDevice",
					) as PlaygroundSettings["embedBrowserDevice"]) || "auto",
			};
			const modelChanged =
				next.embedBrowserModel !== settings.embedBrowserModel ||
				next.embedBrowserDevice !== settings.embedBrowserDevice;
			if (modelChanged) browserDims = null;
			saveSettings(next);

			const note = client.configure(
				settings,
				(msg) => {
					const statusEl = query("[data-embed-status]");
					if (statusEl) statusEl.textContent = msg;
				},
				(dims) => {
					browserDims = dims;
					updateStatusBar();
				},
			);

			const statusEl = query("[data-embed-status]");
			if (statusEl) statusEl.textContent = note;
			updateConnectionDisplay();
			updateStatusBar();
			byId<HTMLDialogElement>("settings-dialog").close();
			showToast("Settings saved");
			void checkEndpoint();
		});

		const testBtn = query("[data-test-connection]", settingsForm);
		if (testBtn) {
			testBtn.addEventListener("click", async () => {
				const probeEl = query("[data-embed-probe]", settingsForm);
				const probeDot = query("[data-embed-probe-dot]", settingsForm);
				if (probeEl) probeEl.textContent = "Pinging endpoint…";
				const target = (urlInput?.value || settings.qdrantUrl).trim();
				const verdict = await probeQdrant(target);
				const setDot = (cls: string) => {
					if (!probeDot) return;
					probeDot.classList.remove("bg-[var(--q-ok)]", "bg-[var(--q-bad)]");
					probeDot.classList.add(cls);
				};
				if (probeEl) {
					if (verdict.kind === "ok") {
						probeEl.textContent = "✓ Connected to Qdrant successfully";
						setDot("bg-[var(--q-ok)]");
					} else if (verdict.kind === "cors") {
						probeEl.textContent = "⚠ Qdrant up but blocked by browser CORS";
						setDot("bg-[var(--q-warn)]");
					} else if (verdict.kind === "http-error") {
						probeEl.textContent = `⚠ Qdrant returned HTTP ${verdict.status}`;
						setDot("bg-[var(--q-warn)]");
					} else {
						probeEl.textContent = "✕ Qdrant unreachable (connection refused)";
						setDot("bg-[var(--q-bad)]");
					}
				}
			});
		}
	}

	// Policy Form
	const policyForm = query<HTMLFormElement>("[data-policy-form]");
	if (policyForm) {
		const enabledInput = policyForm.elements.namedItem(
			"enabled",
		) as HTMLInputElement | null;
		const fieldInput = policyForm.elements.namedItem(
			"field",
		) as HTMLInputElement | null;
		const opSelect = policyForm.elements.namedItem(
			"op",
		) as HTMLSelectElement | null;
		const valueInput = policyForm.elements.namedItem(
			"value",
		) as HTMLInputElement | null;
		const typeSelect = policyForm.elements.namedItem(
			"valueType",
		) as HTMLSelectElement | null;
		const shardInput = policyForm.elements.namedItem(
			"shardKey",
		) as HTMLInputElement | null;

		if (enabledInput) enabledInput.checked = policy.enabled;
		if (fieldInput) fieldInput.value = policy.field;
		if (opSelect) opSelect.value = policy.op;
		if (valueInput) valueInput.value = policy.value;
		if (typeSelect) typeSelect.value = policy.valueType;
		if (shardInput) shardInput.value = policy.shardKey;

		const updatePolicyPreview = (): void => {
			const preview = query("[data-policy-preview]", policyForm);
			if (!preview) return;
			if (!enabledInput?.checked) {
				preview.textContent =
					"Enable injection to rewrite the AST after parse.";
				return;
			}
			const type = typeSelect?.value || "string";
			const raw = valueInput?.value ?? "";
			const literal =
				type === "string" ? `"${raw}"` : type === "boolean" ? raw : raw;
			const shard = shardInput?.value.trim() ?? "";
			const clause = `WHERE ${fieldInput?.value || "tenant_id"} ${opSelect?.value || "="} ${literal}`;
			preview.textContent = shard ? `${clause}\nSHARD ${shard}` : clause;
		};
		for (const input of [enabledInput, fieldInput, valueInput, shardInput]) {
			input?.addEventListener("input", updatePolicyPreview);
		}
		for (const select of [opSelect, typeSelect]) {
			select?.addEventListener("change", updatePolicyPreview);
		}
		updatePolicyPreview();

		policyForm.addEventListener("submit", (e) => {
			e.preventDefault();
			const formData = new FormData(policyForm);
			savePolicy({
				enabled: enabledInput?.checked ?? false,
				field: String(formData.get("field") || "tenant_id").trim(),
				op: String(formData.get("op") || "="),
				value: String(formData.get("value") || "").trim(),
				valueType:
					(formData.get("valueType") as RuntimePolicy["valueType"]) || "string",
				shardKey: String(formData.get("shardKey") || "").trim(),
			});
			byId<HTMLDialogElement>("policy-dialog").close();
			showToast("Tenant policy updated");
			runAnalysis(activeEditor.source());
		});

		// Recipe buttons
		queryAll<HTMLButtonElement>("[data-policy-recipe]", policyForm).forEach(
			(recipeBtn) => {
				recipeBtn.addEventListener("click", () => {
					if (enabledInput) enabledInput.checked = true;
					if (fieldInput && recipeBtn.dataset.field)
						fieldInput.value = recipeBtn.dataset.field;
					if (opSelect && recipeBtn.dataset.op)
						opSelect.value = recipeBtn.dataset.op;
					if (valueInput && recipeBtn.dataset.value)
						valueInput.value = recipeBtn.dataset.value;
					if (typeSelect && recipeBtn.dataset.valueType)
						typeSelect.value = recipeBtn.dataset.valueType;
					if (shardInput && recipeBtn.dataset.shard != null)
						shardInput.value = recipeBtn.dataset.shard;
					updatePolicyPreview();
					showToast(`Loaded policy recipe: ${recipeBtn.dataset.field}`);
				});
			},
		);
	}

	// Exporter logic
	const exportDialog = byId("export-dialog");
	const exportOutput = query("[data-export-output]", exportDialog);
	const exportTabBtns = queryAll<HTMLButtonElement>(
		"[data-export-tab]",
		exportDialog,
	);
	const copyExportBtn = query("[data-copy-export]", exportDialog);

	const updateExport = () => {
		if (!exportOutput) return;
		const source = activeEditor.source();
		const route = selectedRoute(analysis, selectedStmtIndex);
		const count =
			statementSpans.length || analysis?.result.statements_count || 1;
		exportOutput.textContent = exportCode(
			activeLanguage,
			source,
			settings,
			route,
			selectedStmtIndex,
			count,
		);
	};

	exportTabBtns.forEach((btn) => {
		btn.addEventListener("click", () => {
			exportTabBtns.forEach((b) => {
				b.setAttribute("aria-pressed", "false");
			});
			btn.setAttribute("aria-pressed", "true");
			activeLanguage = (btn.dataset.exportTab as ExportLanguage) || "python";
			updateExport();
		});
	});

	if (copyExportBtn && exportOutput) {
		copyExportBtn.addEventListener("click", () => {
			void navigator.clipboard.writeText(exportOutput.textContent || "");
			showToast("Export code copied to clipboard");
		});
	}

	queryAll("[data-open-export]").forEach((btn) => {
		btn.addEventListener("click", updateExport);
	});

	// Connection display
	function updateConnectionDisplay(): void {
		const host = connectionHost(settings.qdrantUrl);
		const connVal = query("[data-connection-value]");
		const fullEl = query("[data-connection-full]");
		const statusEndpoint = query("[data-status-endpoint]");
		if (connVal) connVal.textContent = host;
		if (statusEndpoint) statusEndpoint.textContent = host;
		if (fullEl) fullEl.textContent = settings.qdrantUrl;
	}

	async function checkEndpoint(): Promise<void> {
		const dot = query("[data-connected-dot]");
		const status = query("[data-status-endpoint]");
		const verdict = await probeQdrant(settings.qdrantUrl);
		if (dot && status) {
			dot.classList.remove("is-ready", "is-failed");
			if (verdict.kind === "ok") {
				dot.classList.add("is-ready");
				status.textContent = `${connectionHost(settings.qdrantUrl)} · online`;
			} else {
				dot.classList.add("is-failed");
				status.textContent = `${connectionHost(settings.qdrantUrl)} · unreachable`;
			}
		}

		// Also populate live presets in preset dialog
		try {
			const collections = await listCollections(
				settings.qdrantUrl,
				settings.qdrantKey,
			);
			const liveSection = query("[data-live-section]");
			const livePresets = query("[data-live-presets]");
			const liveHint = query("[data-live-hint]");
			if (liveSection && livePresets && collections.length > 0) {
				if (liveHint)
					liveHint.textContent = `Found ${collections.length} collection(s) on your endpoint:`;
				livePresets.innerHTML = collections
					.map(
						(name) => `
					<button type="button" class="app-chip text-left justify-start" data-preset-query="SCROLL FROM ${escapeHtml(name)} LIMIT 5;" data-preset-label="${escapeHtml(name)}">
						<span class="app-chip__label">Collection</span>
						<span class="app-chip__value font-mono font-bold">${escapeHtml(name)}</span>
					</button>
				`,
					)
					.join("");

				queryAll("[data-preset-query]", livePresets).forEach((b) => {
					b.addEventListener("click", () => {
						const q = b.dataset.presetQuery;
						if (q) {
							activeEditor.setSource(q);
							if (activeFixture)
								activeFixture.textContent =
									b.dataset.presetLabel || "Collection";
							byId<HTMLDialogElement>("preset-dialog").close();
						}
					});
				});
			}
		} catch {
			// ignore live collections fetch errors
		}
	}

	// Share: the whole script travels in `?q=` (same contract as docs links).
	const copyShareLink = async (): Promise<void> => {
		const url = new URL(window.location.href);
		url.searchParams.set("q", activeEditor.source());
		const href = url.toString();
		if (href.length > 8000) {
			showToast(
				"This script is too long to share as a link — use Export instead",
				true,
			);
			return;
		}
		try {
			await navigator.clipboard.writeText(href);
			showToast("Shareable link copied to clipboard");
		} catch {
			showToast("Could not copy the link", true);
		}
	};
	queryAll("[data-share-link]").forEach((btn) => {
		btn.addEventListener("click", () => void copyShareLink());
	});

	// Command palette (⌘K): every action in one place.
	const palette = query<HTMLDialogElement>("#palette-dialog");
	const paletteHandle = palette
		? setupCommandPalette({
				dialog: palette,
				getCommands: () => {
					const source = activeEditor.source();
					const statementCommands = statementSpans.map((span, index) => ({
						group: "Statements",
						label: `${index + 1} · ${statementKeyword(source, span)}`,
						detail: statementPreview(source, span, 70),
						run: () => selectStatement(index, { reveal: true }),
					}));
					return [
						{
							group: "Run",
							label: "Run statement at cursor",
							detail: "Selection, else the statement under the caret",
							hint: isMac ? "⌘↵" : "Ctrl+↵",
							run: () => void runSmart(),
						},
						{
							group: "Run",
							label: "Run all statements",
							detail: "Batch every statement in the script",
							hint: isMac ? "⇧⌘↵" : "Shift+Ctrl+↵",
							run: () => void runAll(),
						},
						{
							group: "Run",
							label: "Run from current statement to end",
							run: () => {
								const target = resolveRunTarget("from");
								if (target) void runTarget(target);
							},
						},
						...statementCommands,
						{
							group: "Editor",
							label: "Format document",
							hint: "Alt+Shift+F",
							run: formatDocument,
						},
						{
							group: "Editor",
							label: wrapEnabled
								? "Disable line wrapping"
								: "Enable line wrapping",
							run: toggleWrap,
						},
						{
							group: "Editor",
							label: "Comment / uncomment selection",
							hint: isMac ? "⌘/" : "Ctrl+/",
							run: () => {
								if (!toggleComment(activeEditor.view)) {
									showToast("Nothing to comment on this line", true);
								}
							},
						},
						{
							group: "Editor",
							label: "Clear editor",
							run: () => {
								activeEditor.setSource("");
								activeEditor.view.focus();
							},
						},
						{
							group: "Workspace",
							label: "Browse examples",
							run: () => byId<HTMLDialogElement>("preset-dialog").showModal(),
						},
						{
							group: "Workspace",
							label: "Database & embeddings setup",
							run: () => byId<HTMLDialogElement>("settings-dialog").showModal(),
						},
						{
							group: "Workspace",
							label: "Tenant policy",
							run: () => byId<HTMLDialogElement>("policy-dialog").showModal(),
						},
						{
							group: "Workspace",
							label: "Export SDK code",
							run: () => {
								updateExport();
								byId<HTMLDialogElement>("export-dialog").showModal();
							},
						},
						{
							group: "Workspace",
							label: "Copy share link",
							run: () => void copyShareLink(),
						},
						{
							group: "Help",
							label: "Keyboard shortcuts",
							run: () =>
								byId<HTMLDialogElement>("shortcuts-dialog").showModal(),
						},
						{
							group: "Help",
							label: "Open documentation",
							run: () => window.open("/docs/", "_blank", "noopener"),
						},
					];
				},
			})
		: null;

	// Global shortcuts outside the editor: ⌘K palette, ⌘↵ run, Escape closes menus.
	queryAll("[data-open-palette]").forEach((btn) => {
		btn.addEventListener("click", () => paletteHandle?.open());
	});
	document.addEventListener("keydown", (event) => {
		const mod = event.metaKey || event.ctrlKey;
		if (event.key === "Escape") {
			setRunMenu(false);
			return;
		}
		if (!mod) return;
		if (document.querySelector("dialog[open]") && !paletteHandle?.isOpen)
			return;
		if (event.key.toLowerCase() === "k") {
			event.preventDefault();
			paletteHandle?.toggle();
			return;
		}
		if (event.key === "Enter" && !activeEditor.view.hasFocus) {
			event.preventDefault();
			void (event.shiftKey ? runAll() : runSmart());
		}
	});
	document.addEventListener("click", (event) => {
		if (!runMenu || runMenu.hidden) return;
		const target = event.target as Node;
		if (!runMenu.contains(target) && !runMenuToggle?.contains(target)) {
			setRunMenu(false);
		}
	});

	setupDialog("shortcuts-dialog", ["[data-open-shortcuts]"]);

	// 9. WASM Initialization
	const runtimeDot = query("[data-runtime-dot]");
	const statusWasm = query("[data-status-wasm]");

	try {
		await initQql();
		client.configure(
			settings,
			(msg) => {
				const statusEl = query("[data-embed-status]");
				if (statusEl) statusEl.textContent = msg;
			},
			(dims) => {
				browserDims = dims;
				updateStatusBar();
			},
		);

		if (runtimeDot) runtimeDot.classList.add("is-ready");
		if (statusWasm) statusWasm.textContent = "WASM Ready";
		if (editorLoading) editorLoading.remove();
		if (runBtn) runBtn.disabled = false;

		updateConnectionDisplay();
		statementSpans = scanStatementSpans(initialDoc);
		syncStatementState();
		runAnalysis(initialDoc);
		updateCursorStatus();
		void checkEndpoint();
	} catch (e) {
		const msg = formatError(e);
		if (runtimeDot) runtimeDot.classList.add("is-failed");
		if (statusWasm) statusWasm.textContent = "WASM Failed";
		if (editorLoading)
			editorLoading.textContent = `Failed to load WebAssembly: ${msg}`;
		showToast(msg, true);
	}
}

// Start
if (typeof window !== "undefined") {
	void start();
}
