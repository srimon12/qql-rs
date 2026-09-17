/**
 * The playground's single mutable state container and its persistence.
 *
 * Persisted fields (settings, policy, run preferences, tab, mobile view, wrap)
 * survive reloads through localStorage with one guarded read/write helper;
 * session fields (analysis, run report, statement map, selection) live only
 * for the page's lifetime. Modules mutate `state` directly and then re-render
 * through `refresh.ts` — there is no second source of truth.
 */
import type { ExecutionReport } from "qql-wasm-current";
import type { PlaygroundEditor } from "../editor/editor";
import {
	DEFAULT_POLICY,
	DEFAULT_RUN_PREFERENCES,
	DEFAULT_SETTINGS,
	INSPECTOR_TAB_KEY,
	type InspectorTab,
	MOBILE_VIEW_KEY,
	type MobileView,
	type PlaygroundSettings,
	type PlaygroundState,
	POLICY_KEY,
	RUN_KEY,
	type RunPreferences,
	type RuntimePolicy,
	SETTINGS_KEY,
	type StatementStatus,
	WRAP_KEY,
} from "./types";

// ── Persistence primitives ───────────────────────────────────────────────────

/** Guarded localStorage read; a corrupt entry falls back instead of throwing. */
function readStored<T>(key: string, fallback: T): T {
	try {
		const raw = localStorage.getItem(key);
		if (raw !== null) return JSON.parse(raw) as T;
	} catch {
		// corrupted entry or storage unavailable — use the fallback
	}
	return fallback;
}

/** Fields added since an entry was written keep their defaults. */
function readStoredMerged<T extends object>(key: string, defaults: T): T {
	return { ...defaults, ...readStored<Partial<T>>(key, {}) };
}

function writeStored(key: string, value: unknown): void {
	try {
		localStorage.setItem(
			key,
			typeof value === "string" ? value : JSON.stringify(value),
		);
	} catch {
		// quota or private-mode failures must never break the playground
	}
}

// ── State ────────────────────────────────────────────────────────────────────

export const state: PlaygroundState = {
	settings: readStoredMerged(SETTINGS_KEY, DEFAULT_SETTINGS),
	policy: readStoredMerged(POLICY_KEY, DEFAULT_POLICY),
	runPrefs: readStoredMerged(RUN_KEY, DEFAULT_RUN_PREFERENCES),
	inspectorTab: readStored<InspectorTab>(INSPECTOR_TAB_KEY, "response"),
	exportLanguage: "python",
	mobileView: readStored<MobileView>(MOBILE_VIEW_KEY, "editor"),
	wrapEnabled: readStored<string>(WRAP_KEY, "off") === "on",

	analysis: null,
	metrics: null,
	selectedStatement: 0,
	response: null,
	executionError: null,
	executeMs: null,
	statementSpans: [],
	executedStatements: [],
	statementStatus: new Map<number, StatementStatus>(),
	browserDims: null,
};

// ── Persisted setters ────────────────────────────────────────────────────────

export function saveSettings(next: PlaygroundSettings): void {
	state.settings = next;
	writeStored(SETTINGS_KEY, next);
}

export function savePolicy(next: RuntimePolicy): void {
	state.policy = next;
	writeStored(POLICY_KEY, next);
}

export function saveRunPrefs(next: RunPreferences): void {
	state.runPrefs = next;
	writeStored(RUN_KEY, next);
}

export function saveInspectorTab(tab: InspectorTab): void {
	state.inspectorTab = tab;
	writeStored(INSPECTOR_TAB_KEY, tab);
}

export function saveMobileView(view: MobileView): void {
	state.mobileView = view;
	writeStored(MOBILE_VIEW_KEY, view);
}

export function saveWrap(enabled: boolean): void {
	state.wrapEnabled = enabled;
	writeStored(WRAP_KEY, enabled ? "on" : "off");
}

// ── Editor handle ────────────────────────────────────────────────────────────

let editorHandle: PlaygroundEditor | null = null;

export function setEditor(editor: PlaygroundEditor): void {
	editorHandle = editor;
}

/** The mounted editor. Only valid after the bootstrap creates it. */
export function editor(): PlaygroundEditor {
	if (!editorHandle) throw new Error("Playground editor is not mounted yet");
	return editorHandle;
}

/** Live document source, or "" before the editor mounts. */
export function sourceText(): string {
	return editorHandle?.source() ?? "";
}

// ── Derived reads ────────────────────────────────────────────────────────────

/** Statement count: the text scan wins once the document has any content. */
export function statementCount(): number {
	return (
		state.statementSpans.length || state.analysis?.result.statements_count || 0
	);
}

/** Result of one statement, mapped through the last run's execution order. */
export function resultForStatement(
	index: number,
): ExecutionReport["results"][number] | null {
	if (!state.response) return null;
	const position = state.executedStatements.indexOf(index);
	if (position < 0) return null;
	return state.response.results[position] ?? null;
}

/**
 * Re-clamp selection and statuses after the statement map changes (the user
 * typed a `;`, deleted a statement, …).
 */
export function syncStatementState(): void {
	const count = state.statementSpans.length;
	for (const index of [...state.statementStatus.keys()]) {
		if (index >= count) state.statementStatus.delete(index);
	}
	state.selectedStatement = Math.min(
		Math.max(0, state.selectedStatement),
		Math.max(0, count - 1),
	);
}
