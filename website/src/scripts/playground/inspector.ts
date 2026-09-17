import { selectedRoute } from "./analysis";
import { fetchCollectionTopology as fetchTopology } from "./client";
import { openDialog } from "./dialogs";
import { renderExplain, renderHighlighted } from "./display";
import { h, required } from "./dom";
import { emptyState, resultSkeleton } from "./empty";
import { renderErrorCard } from "./error-card";
import { type ReportBranch } from "./results";
import { renderExport, syncExportTabs } from "./exporter";
import type {
	InspectorTab,
	LiveCollectionTopology,
} from "../playground-types";
import { applyPreset } from "./presets";
import { renderReport, resultHitCount } from "./results";
import { setValidationBadge } from "./statusbar";
import { hasRunOnce, store } from "./store";
import { setResultCount, switchTab } from "./tabs";

const state = store.state;

const responseRich = required<HTMLElement>("[data-response-rich]");
const responsePre = required<HTMLElement>(`[data-output="response"]`);
const validationBadge = required<HTMLElement>("[data-validation-badge]");
const analysisSummary = required<HTMLElement>("[data-analysis-summary]");
const runButton = required<HTMLButtonElement>("[data-run]");
const exportButton = required<HTMLButtonElement>("[data-open-export]");

export interface InspectorHooks {
	executeQuery: () => void;
	applyUsing: (collection: string, vector: string) => void;
}

let hooks: InspectorHooks | null = null;

/** Bind run-pipeline callbacks; renderers keep their call sites unchanged. */
export function initInspector(next: InspectorHooks): void {
	hooks = next;
}

function runner(): InspectorHooks {
	if (!hooks) throw new Error("Inspector is not initialized.");
	return hooks;
}

function errorHooks(): {
	resolveTopology: (collection: string) => Promise<LiveCollectionTopology>;
	onApplyUsing: (collection: string, vector: string) => void;
	onLoadQuery: (query: string, label: string) => void;
	onOpenSettings: () => void;
} {
	return {
		resolveTopology: (collection) =>
			fetchTopology(
				store.settings.qdrantUrl.replace(/\/+$/, ""),
				store.settings.qdrantKey,
				collection,
			),
		onApplyUsing: (collection, vector) =>
			runner().applyUsing(collection, vector),
		onLoadQuery: (query, label) => applyPreset(query, label),
		onOpenSettings: () => openDialog("#settings-dialog"),
	};
}

function branchFor(source: string): ReportBranch | undefined {
	const limited = /LIMIT\s+\d+\s*;?\s*$/i.exec(source);
	if (!limited) return undefined;
	const filterQuery = `${source.slice(0, limited.index)}WHERE rating >= 4.5 ${limited[0]}`;
	return {
		filterLabel: "WHERE rating >= 4.5",
		filterQuery,
		onLoadFilter: () => applyPreset(filterQuery, "Filtered search"),
		onSeeWire: () => switchTab("wire"),
		onExportPython: () => {
			state.exportLanguage = "python";
			syncExportTabs();
			renderExport();
			openDialog("#export-dialog");
		},
	};
}

function renderResponse(): void {
	responseRich.replaceChildren();
	setResultCount(null);
	const failure = state.executionError;
	if (failure) {
		responseRich.hidden = false;
		responsePre.hidden = true;
		responseRich.append(renderErrorCard(failure, errorHooks()));
		return;
	}
	const report = state.response;
	if (!report || store.qdrant === "down") {
		responseRich.hidden = false;
		responsePre.hidden = true;
		if (store.qdrant === "down") {
			responseRich.append(
				emptyState("offline", {
					label: "Open connection",
					run: () => openDialog("#settings-dialog"),
				}),
			);
			return;
		}
		if (state.analysis?.result.valid && !hasRunOnce()) {
			responseRich.append(resultSkeleton());
			return;
		}
		responseRich.append(
			emptyState(state.analysis?.result.valid ? "result" : "wire", {
				label: "Run the demo query",
				run: () => void runner().executeQuery(),
			}),
		);
		return;
	}
	const hits = resultHitCount(report);
	setResultCount(hits);
	responseRich.hidden = false;
	responsePre.hidden = true;
	responseRich.append(
		renderReport(
			report,
			state.metrics?.executeMs ?? null,
			state.analysis ? branchFor(state.analysis.source) : undefined,
		),
	);
}

export function renderOutputs(): void {
	const analysis = state.analysis;
	const route = selectedRoute(state.analysis, state.selectedStatement);
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

export function renderValidation(): void {
	const analysis = state.analysis;
	const valid = analysis?.result.valid === true && analysis.policyError == null;
	validationBadge.classList.remove("is-loading", "is-valid", "is-invalid");
	validationBadge.classList.add(valid ? "is-valid" : "is-invalid");

	const stmtCount = analysis?.result.statements_count ?? 0;
	required<HTMLElement>("[data-stmt-count]").textContent =
		`${stmtCount} ${stmtCount === 1 ? "stmt" : "stmts"}`;
	const outcome =
		state.response?.ok && resultHitCount(state.response) != null
			? ` → ${resultHitCount(state.response)} hits`
			: "";

	if (!analysis) {
		validationBadge.textContent = "WASM unavailable";
		analysisSummary.textContent =
			"The current qql-wasm package could not load.";
	} else if (analysis.policyError) {
		setValidationBadge("Policy blocked");
		analysisSummary.textContent = analysis.policyError;
	} else if (analysis.result.valid) {
		setValidationBadge("Valid");
		analysisSummary.textContent = `${stmtCount} ${stmtCount === 1 ? "stmt" : "stmts"} · ${analysis.result.tokens.length} tok · ${state.metrics?.parseMs.toFixed(2)} ms${outcome}`;
	} else {
		setValidationBadge("Invalid QQL");
		// analyze reports the first error only (a single `error` field), so
		// render its stable code with a link to the error code reference.
		const code = analysis.result.error?.code;
		const message =
			analysis.result.error?.message ?? "The parser rejected this input.";
		if (code) {
			analysisSummary.replaceChildren();
			analysisSummary.append(
				h(
					"a",
					{
						href: "/docs/reference/error-codes",
						title: "Open the error code reference",
					},
					code,
				),
				`: ${message}`,
			);
		} else {
			analysisSummary.textContent = message;
		}
	}

	runButton.disabled = !valid;
	exportButton.disabled = !valid;
}
