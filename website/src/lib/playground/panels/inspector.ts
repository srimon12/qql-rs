/**
 * Inspector panel: tab visibility, the statement navigator, the plan card and
 * compiled-route list, the code tabs (wire / AST / tokens / explain), metrics,
 * the response panel, and the response context strip.
 *
 * All renderers read the `store` snapshot; none of them mutate state.
 */
import { escapeHtml, query, queryAll } from "../core/dom";
import {
	resultForStatement,
	saveInspectorTab,
	sourceText,
	state,
	statementCount,
} from "../core/store";
import type { InspectorTab } from "../core/types";
import { buildFailure, selectedRoute } from "../core/wasm";
import { statementKeyword } from "../editor/statements";
import {
	highlightJson,
	pretty,
	renderEmpty,
	renderError,
	renderExecResponse,
	renderExplain,
	renderMetrics,
} from "./render";
import {
	statementStatusBadge,
	statementStatusMark,
	statusDotClass,
} from "./statement-nav";

export function renderInspector(): void {
	renderTabs();
	renderStatementNavigator();
	renderPlanPanel();
	renderCodePanels();
	renderMetricsPanel();
	renderResponsePanel();
	renderResponseContext();
}

/** Tab buttons switch the persisted panel and re-render. */
export function setupInspectorTabs(): void {
	queryAll<HTMLElement>("[data-inspector-tab]").forEach((btn) => {
		btn.addEventListener("click", () => {
			const tab = btn.dataset.inspectorTab as InspectorTab;
			if (!tab) return;
			saveInspectorTab(tab);
			renderInspector();
		});
	});
}

function renderTabs(): void {
	queryAll("[data-inspector-tab]").forEach((btn) => {
		const isSelected = btn.dataset.inspectorTab === state.inspectorTab;
		btn.setAttribute("aria-selected", isSelected ? "true" : "false");
		btn.tabIndex = isSelected ? 0 : -1;
	});
	queryAll("[data-inspector-panel]").forEach((panel) => {
		panel.hidden = panel.dataset.inspectorPanel !== state.inspectorTab;
	});
}

/** Select + prev/next for multi-statement scripts; hidden otherwise. */
function renderStatementNavigator(): void {
	const select = query<HTMLSelectElement>("[data-statement-select]");
	const wrap = query("[data-statement-wrap]");
	const count = statementCount();
	if (!select || !wrap) return;
	wrap.hidden = count <= 1;
	if (count <= 1) return;

	const source = sourceText();
	select.innerHTML = state.statementSpans
		.map((span, index) => {
			const kind = source.length > 0 ? statementKeyword(source, span) : "QQL";
			const mark = statementStatusMark(state.statementStatus.get(index));
			const selected = index === state.selectedStatement ? "selected" : "";
			return `<option value="${index}" ${selected}>${index + 1} · ${escapeHtml(kind)}${mark}</option>`;
		})
		.join("");
}

function renderPlanPanel(): void {
	const analysis = state.analysis;
	if (!analysis) return;
	const route = selectedRoute(analysis, state.selectedStatement);
	const count = statementCount();

	const routeLine = query("[data-route-line]");
	if (routeLine) {
		// Redundant with the Plan tab once per-statement navigation appears.
		routeLine.hidden = count > 1;
		routeLine.textContent = route ? `${route.method} ${route.path}` : "";
	}

	const emptyPlan = query("[data-empty-plan]");
	const planCard = query("[data-plan-card]");
	if (!emptyPlan || !planCard) return;

	const hasPlan = Boolean(analysis.result.valid && route);
	emptyPlan.hidden = hasPlan;
	planCard.hidden = !hasPlan;
	if (!hasPlan || !route) return;

	const methodEl = query("[data-route-method]");
	const pathEl = query("[data-route-path]");
	const typeEl = query("[data-route-type]");
	const policyEl = query("[data-route-policy]");
	const countEl = query("[data-routes-count]");

	if (methodEl) methodEl.textContent = route.method;
	if (pathEl) pathEl.textContent = route.path;
	if (typeEl) typeEl.textContent = "Query / Mutation";
	if (policyEl) {
		policyEl.textContent = state.policy.enabled
			? `${state.policy.field} ${state.policy.op} ${state.policy.value}`
			: "Source only";
	}
	if (countEl) countEl.textContent = String(count);

	const listEl = query("[data-routes-list]");
	if (listEl) {
		listEl.innerHTML = analysis.effectiveRoutes
			.map((compiled, index) => {
				const badge = statementStatusBadge(state.statementStatus.get(index));
				return `
					<button type="button" class="route-row" aria-current="${index === state.selectedStatement ? "true" : "false"}" data-select-stmt="${index}">
						<span class="route-row__index">${index + 1}</span>
						<span class="route-row__type">${escapeHtml(compiled.path)}</span>
						<span class="route-row__method">${escapeHtml(compiled.method)}</span>
						${badge ? `<span class="col-start-3 row-start-1 justify-self-end">${badge}</span>` : ""}
					</button>`;
			})
			.join("");
	}
}

/** Wire / AST / Tokens / Explain pre tabs. */
function renderCodePanels(): void {
	const analysis = state.analysis;
	if (!analysis) return;
	const route = selectedRoute(analysis, state.selectedStatement);

	const wireOutput = query(`[data-output="wire"]`);
	if (wireOutput) {
		wireOutput.innerHTML = route
			? highlightJson(pretty(route.payload))
			: "No compiled route available.";
	}

	const astOutput = query(`[data-output="ast"]`);
	if (astOutput) {
		const currentAst = analysis.effectiveAst?.[state.selectedStatement] ?? null;
		astOutput.innerHTML = currentAst
			? highlightJson(pretty(currentAst))
			: "No AST available.";
	}

	const tokensOutput = query(`[data-output="tokens"]`);
	if (tokensOutput) {
		tokensOutput.innerHTML = highlightJson(pretty(analysis.result.tokens));
	}

	const explainText =
		analysis.result.explain ||
		(analysis.policyError ? analysis.policyError : null);
	const explainRich = query("[data-explain-rich]");
	const explainOutput = query(`[data-output="explain"]`);
	if (explainRich) renderExplain(explainRich, explainText);
	if (explainOutput) {
		explainOutput.textContent = explainText || "No explanation available.";
	}
}

function renderMetricsPanel(): void {
	const metricsCards = query("[data-metrics-cards]");
	const metricsOutput = query(`[data-output="metrics"]`);
	const currentRes = resultForStatement(state.selectedStatement);
	const rawTelemetry =
		(state.response as unknown as { telemetry?: Record<string, unknown> })
			?.telemetry ??
		(currentRes as unknown as { telemetry?: Record<string, unknown> })
			?.telemetry ??
		null;

	if (metricsCards) {
		renderMetrics(metricsCards, state.metrics, rawTelemetry);
	}
	if (metricsOutput && state.metrics) {
		metricsOutput.innerHTML = highlightJson(
			pretty({
				...state.metrics,
				telemetry: rawTelemetry,
				reportSummary: state.response
					? {
							ok: state.response.ok,
							succeeded: state.response.succeeded,
							failed: state.response.failed,
							resultsCount: state.response.results?.length ?? 0,
						}
					: null,
			}),
		);
	}
}

function renderResponsePanel(): void {
	const responseRich = query("[data-response-rich]");
	if (!responseRich) return;
	const responsePre = query(`[data-output="response"]`);
	const hitBadge = query("[data-hit-count-badge]");

	const hideBadge = (): void => {
		if (hitBadge) hitBadge.hidden = true;
	};

	if (state.executionError) {
		renderError(responseRich, state.executionError);
		hideBadge();
		if (responsePre) {
			responsePre.textContent = state.executionError.raw;
			responsePre.hidden = true;
		}
		return;
	}

	if (!state.response) {
		renderEmpty(responseRich);
		hideBadge();
		if (responsePre) {
			responsePre.textContent =
				"Run a query against your reachable Qdrant to see live hits.";
			responsePre.hidden = true;
		}
		return;
	}

	if (responsePre) {
		responsePre.innerHTML = highlightJson(pretty(state.response));
		responsePre.hidden = true;
	}

	const res = resultForStatement(state.selectedStatement);
	if (!res) {
		renderEmpty(
			responseRich,
			`Statement ${state.selectedStatement + 1} has not run in this session. Press ⌘↵ (Ctrl+Enter) to run it, or choose Run all statements.`,
		);
		hideBadge();
		return;
	}

	if (res.ok === false) {
		// Batched runs report per-statement failures in-band; render the
		// structured card rather than a raw message.
		renderError(
			responseRich,
			buildFailure(res.message ?? "Statement failed", state.settings.qdrantUrl),
		);
		hideBadge();
		return;
	}

	const badgeCount = renderExecResponse(responseRich, res, state.executeMs);
	if (hitBadge) {
		hitBadge.hidden = badgeCount == null;
		if (badgeCount != null) hitBadge.textContent = String(badgeCount);
	}
}

/** Context strip above the response: which statement the panel is showing. */
function renderResponseContext(): void {
	const bar = query("[data-response-context]");
	if (!bar) return;
	if (!state.response && !state.executionError) {
		bar.hidden = true;
		return;
	}
	bar.hidden = false;

	const label = query("[data-response-context-label]");
	const meta = query("[data-response-context-meta]");
	const dot = query("[data-response-context-dot]");

	const count = Math.max(1, statementCount());
	const span = state.statementSpans[state.selectedStatement];
	const keyword = span ? statementKeyword(sourceText(), span) : "";
	if (label) {
		label.textContent = span
			? `Statement ${state.selectedStatement + 1} of ${count} · ${keyword}`
			: `Statement ${state.selectedStatement + 1} of ${count}`;
	}

	const status = state.statementStatus.get(state.selectedStatement);
	if (dot) {
		dot.className = `size-1.5 shrink-0 rounded-full ${statusDotClass(status)}`;
	}

	const res = resultForStatement(state.selectedStatement);
	if (!meta) return;
	if (state.executionError && status === "error") {
		meta.textContent = state.executionError.code ?? "Run failed";
	} else if (!res) {
		meta.textContent = count > 1 ? "Not run · ⌘↵ runs this statement" : "";
	} else if (!res.ok) {
		meta.textContent = "Failed · see the result card";
	} else {
		const timing =
			state.executedStatements.length === 1 && state.executeMs !== null
				? ` · ${state.executeMs.toFixed(1)} ms`
				: "";
		meta.textContent = `${res.operation}${timing}`;
	}
}
