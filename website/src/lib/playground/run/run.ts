/**
 * Run engine. Resolves what ⌘↵ (and each run-menu entry) should execute —
 * selection, statement under the caret, the exact line, one statement, all of
 * them, or from here to the end — then executes it, maps the per-statement
 * results back onto the editor, and owns the run menu + error policy.
 */
import { forceLinting } from "@codemirror/lint";
import { query, queryAll, showToast } from "../core/dom";
import { refreshInspector, refreshStatementUi } from "../core/refresh";
import {
	editor,
	saveInspectorTab,
	saveRunPrefs,
	sourceText,
	state,
} from "../core/store";
import { buildFailure, byteOffsetToPosition } from "../core/wasm";
import { runAnalysis } from "../editor/analysis";
import {
	lineAt,
	statementIndexAt,
	statementIndicesInRange,
	statementSource,
} from "../editor/statements";
import { client } from "../services/connection";

export type RunMode = "smart" | "all" | "statement" | "from";

interface RunTarget {
	text: string;
	/** Editor statement indices this run covers, in execution order. */
	statements: number[];
	/** Human label for toasts: "statement 2", "selection", "3 statements". */
	label: string;
}

// ── Target resolution ────────────────────────────────────────────────────────

function resolveRunTarget(mode: RunMode): RunTarget | null {
	const source = sourceText();
	const spans = state.statementSpans;
	if (source.trim().length === 0) return null;

	if (mode === "all") {
		return {
			text: source,
			statements: spans.map((_, index) => index),
			label: spans.length === 1 ? "1 statement" : `${spans.length} statements`,
		};
	}

	if (mode === "smart") {
		const selection = editor().selectionText();
		if (selection) {
			const range = editor().view.state.selection.main;
			return {
				text: selection,
				statements: statementIndicesInRange(spans, range.from, range.to),
				label: "selection",
			};
		}
	}

	const index = editor().cursorStatement();
	if (index < 0 || index >= spans.length) {
		// Comment-only or broken document: run the exact line under the cursor.
		const line = lineAt(source, editor().view.state.selection.main.head);
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
}

// ── Execution ────────────────────────────────────────────────────────────────

function setBusy(busy: boolean): void {
	const runBtn = query<HTMLButtonElement>("[data-run]");
	if (!runBtn) return;
	runBtn.disabled = busy || !client.ready;
	runBtn.toggleAttribute("aria-busy", busy);
	const runLabel = query("[data-run-label]");
	if (runLabel) runLabel.textContent = busy ? "Running…" : "Run";
}

/** Run readiness/idle state on the toolbar button (bootstrap calls this after init). */
export function setRunBusy(busy: boolean): void {
	setBusy(busy);
}

/** Map an in-band failed result back to the statement that produced it. */
function applyRunReport(
	results: NonNullable<typeof state.response>["results"],
	target: RunTarget,
): void {
	state.executedStatements = target.statements.slice(0, results.length);
	state.executedStatements.forEach((statementIndex, position) => {
		state.statementStatus.set(
			statementIndex,
			results[position]?.ok ? "ok" : "error",
		);
	});
	for (const index of target.statements.slice(results.length)) {
		state.statementStatus.set(index, "skipped");
	}
	const firstFailure = state.executedStatements.find(
		(index) => state.statementStatus.get(index) === "error",
	);
	if (firstFailure !== undefined) state.selectedStatement = firstFailure;
	else if (state.executedStatements.length > 0) {
		state.selectedStatement = state.executedStatements[0];
	}
}

/**
 * A stopped multi-statement run reports one error; its span points at the
 * failing statement. Script parse errors surface through the analyzer instead,
 * so fall back to its first diagnostic span; with neither, only the first
 * statement is claimed.
 */
function failingStatementIndex(target: RunTarget): number {
	const span =
		state.executionError?.span ??
		(() => {
			const first =
				state.analysis?.result.errors?.[0] ?? state.analysis?.result.error;
			return first && first.start != null && first.end != null
				? { start: first.start, end: first.end }
				: null;
		})();
	if (!span) return -1;
	return statementIndexAt(
		state.statementSpans,
		byteOffsetToPosition(target.text, span.start),
	);
}

function applyRunFailure(target: RunTarget): void {
	state.executedStatements = [];
	const failingIndex = failingStatementIndex(target);
	if (target.statements.length <= 1) {
		for (const index of target.statements) {
			state.statementStatus.set(index, "error");
		}
		return;
	}
	const stopAt =
		failingIndex >= 0 ? target.statements.indexOf(failingIndex) : 0;
	target.statements.forEach((index, position) => {
		state.statementStatus.set(
			index,
			position < stopAt ? "ok" : position === stopAt ? "error" : "skipped",
		);
	});
}

async function runTarget(target: RunTarget): Promise<void> {
	if (state.policy.enabled && target.statements.length > 1) {
		showToast(
			"Tenant policy applies to one statement at a time — run a single statement or disable the policy",
			true,
		);
		return;
	}
	for (const index of target.statements) {
		state.statementStatus.set(index, "running");
	}
	refreshStatementUi();
	setBusy(true);
	const started = performance.now();
	try {
		state.executionError = null;
		state.response = await client.execute(target.text, state.policy, {
			onError: state.runPrefs.stopOnError ? "stop" : "continue",
		});
		state.executeMs = performance.now() - started;
		applyRunReport(state.response.results, target);
		saveInspectorTab("response");
		refreshStatementUi();
		refreshInspector();

		const failed = state.response.failed ?? 0;
		if (failed === 0) showToast(`Ran ${target.label}`);
		else {
			showToast(
				`${failed} of ${state.response.results.length} statements failed`,
				true,
			);
		}
	} catch (error) {
		state.response = null;
		state.executeMs = performance.now() - started;
		state.executionError = buildFailure(error, state.settings.qdrantUrl);
		applyRunFailure(target);
		saveInspectorTab("response");
		refreshStatementUi();
		refreshInspector();
		showToast(state.executionError.message, true);
	} finally {
		setBusy(false);
		runAnalysis(sourceText());
		forceLinting(editor().view);
	}
}

export async function runSmart(): Promise<void> {
	const target = resolveRunTarget("smart");
	if (target) await runTarget(target);
}

export async function runAll(): Promise<void> {
	const target = resolveRunTarget("all");
	if (target) await runTarget(target);
}

/** Menu + palette entry point for a specific mode. */
export async function runMode(mode: RunMode): Promise<void> {
	const target = resolveRunTarget(mode);
	if (target) await runTarget(target);
}

// ── Run menu ─────────────────────────────────────────────────────────────────

let runMenu: HTMLElement | null = null;
let runMenuToggle: HTMLButtonElement | null = null;

function setRunMenu(open: boolean): void {
	if (!runMenu || !runMenuToggle) return;
	runMenu.hidden = !open;
	runMenuToggle.setAttribute("aria-expanded", String(open));
}

export function closeRunMenu(): void {
	setRunMenu(false);
}

export function setupRunEngine(): void {
	runMenu = query("[data-run-menu]");
	runMenuToggle = query<HTMLButtonElement>("[data-run-menu-toggle]");

	query("[data-run]")?.addEventListener("click", () => void runSmart());
	runMenuToggle?.addEventListener("click", (event) => {
		event.stopPropagation();
		setRunMenu(runMenu ? runMenu.hidden !== false : true);
	});
	queryAll("[data-run-mode]").forEach((btn) => {
		btn.addEventListener("click", () => {
			setRunMenu(false);
			void runMode(btn.dataset.runMode as RunMode);
		});
	});

	const stopOnErrorInput = query<HTMLInputElement>("[data-run-stop-on-error]");
	if (stopOnErrorInput) {
		stopOnErrorInput.checked = state.runPrefs.stopOnError;
		stopOnErrorInput.addEventListener("change", () => {
			saveRunPrefs({ stopOnError: stopOnErrorInput.checked });
			showToast(
				stopOnErrorInput.checked
					? "Multi-statement runs stop on the first error"
					: "Multi-statement runs continue past errors",
			);
		});
	}

	// Clicking anywhere outside the menu closes it; Escape is handled globally.
	document.addEventListener("click", (event) => {
		if (!runMenu || runMenu.hidden) return;
		const target = event.target as Node;
		if (!runMenu.contains(target) && !runMenuToggle?.contains(target)) {
			setRunMenu(false);
		}
	});
}
