/**
 * Document lifecycle: analysis (parse + plan + policy rewrite), the debounced
 * edit pipeline that persists the working document, editor diagnostics, and
 * loading a new document into the editor.
 */
import type { Diagnostic } from "@codemirror/lint";
import { query } from "../core/dom";
import { refreshAll, refreshStatementUi } from "../core/refresh";
import { editor, state, syncStatementState } from "../core/store";
import { WORKSPACE_KEY } from "../core/types";
import {
	analyzeWithPolicy,
	currentDiagnostic,
	runtimeDiagnostic,
} from "../core/wasm";
import { scanStatementSpans } from "./statements";

/** Linter source: static analysis errors plus the last runtime failure span. */
export function editorDiagnostics(): Diagnostic[] {
	return [
		...currentDiagnostic(state.analysis),
		...runtimeDiagnostic(state.executionError, editor().source()),
	];
}

/** Force a full parse/plan pass and re-render everything. */
export function runAnalysis(source: string): void {
	state.statementSpans = scanStatementSpans(source);
	const out = analyzeWithPolicy(
		source,
		state.policy,
		state.settings.embedProvider,
		state.executeMs,
	);
	state.analysis = out.analysis;
	state.metrics = out.metrics;
	refreshAll();
}

let debounceTimer: number | null = null;

/**
 * Edit pipeline: refresh the statement map immediately, then debounce the
 * analysis and the localStorage snapshot (80 ms) so typing stays smooth.
 */
export function queueAnalysis(source: string): void {
	state.statementSpans = scanStatementSpans(source);
	syncStatementState();
	refreshStatementUi();
	if (debounceTimer) window.clearTimeout(debounceTimer);
	debounceTimer = window.setTimeout(() => {
		runAnalysis(source);
		persistWorkspace(source);
	}, 80);
}

function persistWorkspace(source: string): void {
	try {
		localStorage.setItem(WORKSPACE_KEY, source);
	} catch {
		// storage quota — losing the snapshot is harmless
	}
}

/** Update the "Example: …" label in the editor toolbar. */
export function setFixtureLabel(label: string): void {
	const el = query("[data-active-fixture]");
	if (el) el.textContent = label;
}

/** Drop the previous run: its results and per-statement marks no longer match. */
export function resetRunState(): void {
	state.response = null;
	state.executionError = null;
	state.executeMs = null;
	state.executedStatements = [];
	state.statementStatus.clear();
}

/**
 * Load a document into the editor (examples, live collections, injected
 * queries): clears stale run output, then runs the normal edit pipeline.
 */
export function loadSource(source: string, fixtureLabel?: string): void {
	resetRunState();
	editor().setSource(source);
	if (fixtureLabel) setFixtureLabel(fixtureLabel);
	queueAnalysis(source);
}
