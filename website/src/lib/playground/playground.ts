import { indentWithTab } from "@codemirror/commands";
import { linter, lintGutter } from "@codemirror/lint";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { basicSetup } from "codemirror";
import type { ExecutionReport } from "qql-wasm-current";
import { listCollections, probeQdrant, QdrantClient } from "./client";
import { exportCode } from "./export";
import { qqlCompletion, qqlHighlighting, qqlLanguage } from "./language";
import {
	escapeHtml,
	highlightJson,
	pretty,
	renderCount,
	renderEmpty,
	renderError,
	renderFacets,
	renderHits,
	renderMutation,
	type ScoredPoint,
} from "./render";
import {
	DEFAULT_POLICY,
	DEFAULT_SETTINGS,
	type ExportLanguage,
	INSPECTOR_TAB_KEY,
	type InspectorTab,
	type PlaygroundAnalysis,
	type PlaygroundMetrics,
	type PlaygroundSettings,
	POLICY_KEY,
	type RuntimePolicy,
	SETTINGS_KEY,
	SPLIT_KEY,
	WORKSPACE_KEY,
} from "./types";
import {
	analyzeWithPolicy,
	buildFailure,
	connectionHost,
	currentDiagnostic,
	formatError,
	formatQuery,
	initQql,
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

let settings: PlaygroundSettings = loadSettings();
let policy: RuntimePolicy = loadPolicy();
let activeTab: InspectorTab = loadTab();
let activeLanguage: ExportLanguage = "python";
let analysis: PlaygroundAnalysis | null = null;
let metrics: PlaygroundMetrics | null = null;
let selectedStmtIndex = 0;
let lastExecutionReport: ExecutionReport | null = null;
let lastExecutionError: ReturnType<typeof buildFailure> | null = null;
let lastExecuteMs: number | null = null;

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
		if (settings.embedProvider === "browser")
			embedChip.textContent = "MiniLM (browser)";
		else if (settings.embedProvider === "http")
			embedChip.textContent = `${settings.embedModel || "HTTP"}`;
		else embedChip.textContent = "None (raw vectors)";
	}
	if (embedDimChip) {
		embedDimChip.hidden = settings.embedProvider === "none";
		embedDimChip.textContent = `${settings.embedDim}d`;
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
		const count = analysis.result.statements_count;
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

	// 2. Update Statement Dropdown in inspector header
	const stmtSelect = query<HTMLSelectElement>("[data-statement-select]");
	const stmtWrap = query("[data-statement-wrap]");
	const count = analysis.result.statements_count;
	if (stmtSelect && stmtWrap) {
		if (count > 1) {
			stmtWrap.hidden = false;
			stmtSelect.disabled = false;
			stmtSelect.hidden = false;
			stmtSelect.innerHTML = Array.from({ length: count })
				.map(
					(_, i) =>
						`<option value="${i}" ${i === selectedStmtIndex ? "selected" : ""}>Statement ${i + 1}</option>`,
				)
				.join("");
		} else {
			stmtWrap.hidden = true;
			stmtSelect.disabled = true;
			stmtSelect.hidden = true;
		}
	}

	const route = selectedRoute(analysis, selectedStmtIndex);
	const routeLine = query("[data-route-line]");
	if (routeLine) {
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
					.map(
						(r, idx) => `
					<button type="button" class="route-row ${idx === selectedStmtIndex ? 'aria-current="true"' : ""}" data-select-stmt="${idx}">
						<span class="route-row__index">${idx + 1}</span>
						<span class="route-row__type">${escapeHtml(r.path)}</span>
						<span class="route-row__method">${escapeHtml(r.method)}</span>
					</button>
				`,
					)
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

	const explainOutput = query(`[data-output="explain"]`);
	if (explainOutput) {
		explainOutput.textContent =
			analysis.result.explain ||
			(analysis.policyError
				? analysis.policyError
				: "No explanation available.");
	}

	const metricsOutput = query(`[data-output="metrics"]`);
	if (metricsOutput && metrics) {
		metricsOutput.innerHTML = highlightJson(pretty(metrics));
	}

	// 5. Update Response Tab
	const responseRich = query("[data-response-rich]");
	const responsePre = query(`[data-output="response"]`);

	if (responseRich) {
		if (lastExecutionError) {
			renderError(responseRich, lastExecutionError);
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
			// Inspect response type
			const hits = report.hits
				? (report.hits(selectedStmtIndex) as unknown as ScoredPoint[])
				: [];
			if (hits && hits.length > 0) {
				renderHits(responseRich, hits, lastExecuteMs);
			} else if (
				report.count &&
				typeof report.count(selectedStmtIndex) === "number"
			) {
				renderCount(
					responseRich,
					report.count(selectedStmtIndex),
					lastExecuteMs,
				);
			} else if (report.facet && report.facet(selectedStmtIndex)?.length > 0) {
				renderFacets(
					responseRich,
					report.facet(selectedStmtIndex),
					lastExecuteMs,
				);
			} else {
				const firstRes = report.results?.[selectedStmtIndex];
				if (firstRes) {
					renderMutation(
						responseRich,
						firstRes.operation,
						firstRes.message,
						lastExecuteMs,
					);
				} else {
					renderEmpty(responseRich, "Query executed successfully.");
				}
			}
		} else {
			renderEmpty(responseRich);
			if (responsePre) {
				responsePre.textContent =
					"Run a query against your reachable Qdrant to see live hits.";
				responsePre.hidden = true;
			}
		}
	}
}

// ── Main Start Routine ───────────────────────────────────────────────────────

export async function start(): Promise<void> {
	const workspace = query<HTMLElement>("[data-workspace]");
	const editorHost = byId("qql-editor");
	const runBtn = query<HTMLButtonElement>("[data-run]");
	const formatBtn = query<HTMLButtonElement>("[data-format]");
	const activeFixture = query("[data-active-fixture]");
	const editorLoading = query("[data-editor-loading]");

	// 1. Initial document
	const savedSource = localStorage.getItem(WORKSPACE_KEY);
	const initialDoc =
		savedSource ||
		workspace?.dataset.defaultQuery ||
		"QUERY points FROM collection LIMIT 5;";

	// 2. Editor setup
	let editor: EditorView;
	const runAnalysis = (source: string) => {
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
	};

	let debounceTimer: number | null = null;
	const queueAnalysis = (source: string) => {
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

	editor = new EditorView({
		state: EditorState.create({
			doc: initialDoc,
			extensions: [
				basicSetup,
				qqlLanguage,
				qqlCompletion,
				qqlHighlighting,
				lintGutter(),
				linter(() => currentDiagnostic(analysis)),
				placeholder("-- Type your QQL vector query here…"),
				keymap.of([
					indentWithTab,
					{
						key: "Mod-Enter",
						run: () => {
							void execute();
							return true;
						},
					},
				]),
				EditorView.updateListener.of((update) => {
					if (update.docChanged) {
						if (activeFixture) activeFixture.textContent = "Custom query";
						queueAnalysis(update.state.doc.toString());
					}
				}),
			],
		}),
		parent: editorHost,
	});

	// 3. Execution logic
	async function execute(): Promise<void> {
		if (!runBtn) return;
		const source = editor.state.doc.toString().trim();
		if (!source) return;

		runBtn.disabled = true;
		runBtn.setAttribute("aria-busy", "true");
		const runLabel = query("[data-run-label]");
		if (runLabel) runLabel.textContent = "Running…";

		const started = performance.now();
		try {
			lastExecutionError = null;
			lastExecutionReport = await client.execute(source, policy);
			lastExecuteMs = performance.now() - started;
			showToast("Query completed successfully");
			// Switch to response tab on successful run
			activeTab = "response";
			saveTab("response");
		} catch (error) {
			lastExecutionReport = null;
			lastExecuteMs = performance.now() - started;
			lastExecutionError = buildFailure(error, settings.qdrantUrl);
			showToast(lastExecutionError.message, true);
			activeTab = "response";
			saveTab("response");
		} finally {
			runBtn.disabled = false;
			runBtn.removeAttribute("aria-busy");
			if (runLabel) runLabel.textContent = "Run";
			runAnalysis(editor.state.doc.toString());
		}
	}

	if (runBtn) {
		runBtn.addEventListener("click", () => void execute());
	}

	// 4. Format logic
	if (formatBtn) {
		formatBtn.addEventListener("click", () => {
			const source = editor.state.doc.toString();
			try {
				const formatted = formatQuery(source);
				if (formatted && formatted !== source) {
					editor.dispatch({
						changes: { from: 0, to: source.length, insert: formatted },
					});
					showToast("Query formatted");
				}
			} catch (e) {
				showToast(formatError(e), true);
			}
		});
	}

	// 5. Wrap toggle
	const wrapBtn = query<HTMLButtonElement>("[data-wrap-toggle]");
	if (wrapBtn) {
		let isWrap = false;
		wrapBtn.addEventListener("click", () => {
			isWrap = !isWrap;
			editorHost.dataset.wrap = isWrap ? "on" : "off";
			wrapBtn.setAttribute("aria-pressed", isWrap ? "true" : "false");
			editor.requestMeasure();
		});
	}

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
				editor.requestMeasure();
				splitHandle.removeEventListener("pointermove", onMove);
				splitHandle.removeEventListener("pointerup", onUp);
			};
			splitHandle.addEventListener("pointermove", onMove);
			splitHandle.addEventListener("pointerup", onUp);
		});

		splitHandle.addEventListener("dblclick", () => {
			applySplit(1.08);
			localStorage.setItem(SPLIT_KEY, "1.08");
			editor.requestMeasure();
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

	// Statement select change
	const stmtSelect = query<HTMLSelectElement>("[data-statement-select]");
	if (stmtSelect) {
		stmtSelect.addEventListener("change", () => {
			selectedStmtIndex = Number(stmtSelect.value) || 0;
			updateInspectorPanels();
		});
	}

	// Copy point JSON delegate
	document.addEventListener("click", (e) => {
		const btn = (e.target as HTMLElement).closest<HTMLButtonElement>(
			"[data-copy-json]",
		);
		if (btn?.dataset.copyJson) {
			void navigator.clipboard.writeText(btn.dataset.copyJson);
			showToast("Copied JSON to clipboard");
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
	const presetDialog = byId("preset-dialog");
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
				editor.dispatch({
					changes: { from: 0, to: editor.state.doc.length, insert: q },
				});
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

		if (urlInput) urlInput.value = settings.qdrantUrl;
		if (keyInput) keyInput.value = settings.qdrantKey;
		if (providerSelect) providerSelect.value = settings.embedProvider;
		if (embedUrlInput) embedUrlInput.value = settings.embedUrl;
		if (embedModelInput) embedModelInput.value = settings.embedModel;
		if (embedDimInput) embedDimInput.value = String(settings.embedDim);
		if (embedKeyInput) embedKeyInput.value = settings.embedKey;

		settingsForm.addEventListener("submit", (e) => {
			e.preventDefault();
			const formData = new FormData(settingsForm);
			saveSettings({
				qdrantUrl: String(
					formData.get("qdrantUrl") || DEFAULT_SETTINGS.qdrantUrl,
				).trim(),
				qdrantKey: String(formData.get("qdrantKey") || "").trim(),
				embedProvider:
					(formData.get(
						"embedProvider",
					) as PlaygroundSettings["embedProvider"]) || "browser",
				embedUrl: String(
					formData.get("embedUrl") || DEFAULT_SETTINGS.embedUrl,
				).trim(),
				embedModel: String(
					formData.get("embedModel") || DEFAULT_SETTINGS.embedModel,
				).trim(),
				embedDim: Number(formData.get("embedDim")) || DEFAULT_SETTINGS.embedDim,
				embedKey: String(formData.get("embedKey") || "").trim(),
			});

			client.configure(settings, (msg) => {
				const statusEl = query("[data-embed-status]");
				if (statusEl) statusEl.textContent = msg;
			});

			updateConnectionDisplay();
			byId<HTMLDialogElement>("settings-dialog").close();
			showToast("Settings saved");
			void checkEndpoint();
		});

		const testBtn = query("[data-test-connection]", settingsForm);
		if (testBtn) {
			testBtn.addEventListener("click", async () => {
				const probeEl = query("[data-embed-probe]", settingsForm);
				if (probeEl) probeEl.textContent = "Pinging endpoint…";
				const target = (urlInput?.value || settings.qdrantUrl).trim();
				const verdict = await probeQdrant(target);
				if (probeEl) {
					if (verdict.kind === "ok")
						probeEl.textContent = "✓ Connected to Qdrant successfully";
					else if (verdict.kind === "cors")
						probeEl.textContent = "⚠ Qdrant up but blocked by browser CORS";
					else if (verdict.kind === "http-error")
						probeEl.textContent = `⚠ Qdrant returned HTTP ${verdict.status}`;
					else
						probeEl.textContent = "✕ Qdrant unreachable (connection refused)";
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
			runAnalysis(editor.state.doc.toString());
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
		const source = editor.state.doc.toString();
		const route = selectedRoute(analysis, selectedStmtIndex);
		const count = analysis?.result.statements_count ?? 1;
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
					<button type="button" class="app-chip text-left justify-start" data-preset-query="QUERY points FROM ${escapeHtml(name)} LIMIT 5;" data-preset-label="${escapeHtml(name)}">
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
							editor.dispatch({
								changes: { from: 0, to: editor.state.doc.length, insert: q },
							});
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

	// Share functionality
	const shareBtn = query("[data-overflow-share]");
	const shareMenu = query<HTMLElement>("[data-share-menu]");
	const shareUrlInput = query<HTMLInputElement>("[data-share-url]");
	const shareCopyBtn = query("[data-share-copy]");

	if (shareBtn && shareMenu && shareUrlInput) {
		shareBtn.addEventListener("click", () => {
			const url = new URL(window.location.href);
			url.searchParams.set("q", editor.state.doc.toString());
			shareUrlInput.value = url.toString();
		});
	}
	if (shareCopyBtn && shareUrlInput) {
		shareCopyBtn.addEventListener("click", () => {
			void navigator.clipboard.writeText(shareUrlInput.value);
			shareCopyBtn.setAttribute("data-copied", "true");
			showToast("Share URL copied to clipboard");
			window.setTimeout(
				() => shareCopyBtn.removeAttribute("data-copied"),
				2000,
			);
		});
	}

	// 9. WASM Initialization
	const runtimeDot = query("[data-runtime-dot]");
	const statusWasm = query("[data-status-wasm]");

	try {
		await initQql();
		client.configure(settings, (msg) => {
			const statusEl = query("[data-embed-status]");
			if (statusEl) statusEl.textContent = msg;
		});

		if (runtimeDot) runtimeDot.classList.add("is-ready");
		if (statusWasm) statusWasm.textContent = "WASM Ready";
		if (editorLoading) editorLoading.remove();
		if (runBtn) runBtn.disabled = false;

		updateConnectionDisplay();
		runAnalysis(initialDoc);
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
