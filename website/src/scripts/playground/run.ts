import type { EditorView } from "@codemirror/view";
import { forceLinting } from "@codemirror/lint";
import { analyzeWithPolicy, fallbackAnalysis } from "./analysis";
import { pool } from "./client";
import { markCustomQuery, replaceDoc, tryFormat } from "./editor";
import { resultSkeleton } from "./empty";
import { buildFailure, formatError } from "./errors";
import { renderOutputs, renderValidation } from "./inspector";
import { renderInspector } from "./plan";
import { markRunOnce, store } from "./store";
import { setFirstRunPing, setRuntime } from "./statusbar";
import { setResultCount, switchTab } from "./tabs";
import { toast, toastFailure } from "./toasts";
import { required } from "./dom";

const state = store.state;

let boundEditor: EditorView | null = null;
let boundFixture: HTMLElement | null = null;

function editor(): EditorView {
	if (!boundEditor) throw new Error("Run pipeline is not initialized.");
	return boundEditor;
}

function fixture(): HTMLElement {
	if (!boundFixture) throw new Error("Run pipeline is not initialized.");
	return boundFixture;
}

/** Bind once at boot; run entry points keep their call sites unchanged. */
export function initRun(next: EditorView, label: HTMLElement): void {
	boundEditor = next;
	boundFixture = label;
}

const runButton = required<HTMLButtonElement>("[data-run]");
const runLabel = required<HTMLElement>("[data-run-label]");
const responseRich = required<HTMLElement>("[data-response-rich]");
const responsePre = required<HTMLElement>(`[data-output="response"]`);


const ANALYSIS_DELAY_MS = 90;

let analysisTimer: number | null = null;
export function applyUsing(collection: string, vector: string): void {
	const source = editor().state.doc.toString();
	const clause = `USING ${vector} AS DENSE`;
	let next: string;
	if (/USING\s/i.test(source)) {
		next = source.replace(/USING\s+\S+(\s+AS\s+\S+)?/i, clause);
	} else {
		const anchor = `FROM ${collection} `;
		const at = source.toUpperCase().indexOf(anchor.toUpperCase());
		if (at === -1) {
			toast(`Couldn't find FROM ${collection} in the editor.`, "error");
			return;
		}
		const insertAt = at + anchor.length;
		next = `${source.slice(0, insertAt)}${clause} ${source.slice(insertAt)}`;
	}
	replaceDoc(editor(), next);
	markCustomQuery(fixture());
	editor().focus();
	toast(`Applied ${clause} — Run again.`);
}

export function formatEditor(): void {
	const source = editor().state.doc.toString();
	const outcome = tryFormat(source);
	if (outcome.kind === "unchanged") {
		toast("Already formatted.");
	} else if (outcome.kind === "formatted") {
		replaceDoc(editor(), outcome.text);
		editor().focus();
		toast("Formatted with the canonical formatter.");
	} else {
		toast(outcome.message, "error");
	}
}

export function runAnalysis(source: string): void {
	try {
		const { analysis, metrics } = analyzeWithPolicy(
			source,
			store.policy,
			store.settings.embedProvider,
			state.metrics?.executeMs ?? null,
		);
		state.analysis = analysis;
		state.metrics = metrics;
	} catch (error) {
		state.analysis = fallbackAnalysis(source, formatError(error));
	}
	state.selectedStatement = 0;
	renderInspector();
	forceLinting(editor());
}

export function queueAnalysis(source: string): void {
	if (analysisTimer != null) window.clearTimeout(analysisTimer);
	analysisTimer = window.setTimeout(() => {
		analysisTimer = null;
		runAnalysis(source);
	}, ANALYSIS_DELAY_MS);
}

export async function executeQuery(): Promise<void> {
	if (
		!pool.ready ||
		!state.analysis?.result.valid ||
		state.analysis.policyError
	) {
		return;
	}
	runButton.disabled = true;
	runButton.classList.add("is-running");
	runLabel.textContent = "Running…";
	state.executionError = null;
	state.response = null;
	setResultCount(null);
	responseRich.replaceChildren();
	responseRich.hidden = false;
	responsePre.hidden = true;
	responseRich.append(resultSkeleton());
	setRuntime("Executing against Qdrant…");

	const source = editor().state.doc.toString();
	const started = performance.now();

	try {
		state.response = await pool.execute(source, store.policy);
		if (state.metrics) state.metrics.executeMs = performance.now() - started;
		setRuntime(
			state.response.ok
				? `Execution complete, ${state.response.succeeded} succeeded`
				: `Execution complete, ${state.response.failed} failed`,
			state.response.ok ? "ready" : "failed",
		);
		if (state.response.ok) {
			markRunOnce();
			setFirstRunPing(false);
		}
		switchTab("response");
		toast(
			state.response.ok ? "Query executed." : "Qdrant returned a failure.",
			state.response.ok ? "success" : "error",
		);
	} catch (error) {
		state.executionError = buildFailure(error, store.settings.qdrantUrl);
		if (state.metrics) state.metrics.executeMs = performance.now() - started;
		setRuntime("Execution failed.", "failed");
		switchTab("response");
		toastFailure(state.executionError);
	} finally {
		runButton.classList.remove("is-running");
		runLabel.textContent = "Run";
		renderValidation();
		renderOutputs();
	}
}

