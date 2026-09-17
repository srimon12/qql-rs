/**
 * Playground bootstrap: resolves the initial document, mounts the editor, wires
 * every surface to its module, then starts the WASM runtime and the first
 * analysis pass. This is the only module that knows about all the others.
 */

import {
	applyPlatformKeyLabels,
	byId,
	query,
	setupDialog,
	showToast,
} from "./core/dom";
import { setEditor, state, syncStatementState } from "./core/store";
import { WORKSPACE_KEY } from "./core/types";
import { formatError, initQql } from "./core/wasm";
import { setupExporter } from "./dialogs/export";
import { setupPolicyForm } from "./dialogs/policy";
import { setupPresets } from "./dialogs/presets";
import { setupSettingsForm } from "./dialogs/settings";
import {
	editorDiagnostics,
	queueAnalysis,
	runAnalysis,
	setFixtureLabel,
} from "./editor/analysis";
import { createPlaygroundEditor } from "./editor/editor";
import {
	applyWrap,
	formatDocument,
	setupEditorActions,
} from "./editor/editor-actions";
import {
	followCursor,
	selectStatement,
	setupStatementNav,
} from "./editor/selection";
import { scanStatementSpans } from "./editor/statements";
import { setupInspectorTabs } from "./panels/inspector";
import { renderCursorStatus } from "./panels/statement-nav";
import { runAll, runSmart, setRunBusy, setupRunEngine } from "./run/run";
import {
	checkEndpoint,
	configureClient,
	renderConnectionDisplay,
	setEmbedStatus,
} from "./services/connection";
import { buildCommands } from "./shell/commands";
import { setupDocumentDelegates } from "./shell/delegates";
import {
	applyMobileView,
	setupMobileViewSwitch,
	setupSplitHandle,
} from "./shell/layout";
import { type CommandPalette, setupCommandPalette } from "./shell/palette";
import { setupGlobalShortcuts } from "./shell/shortcuts";

const FALLBACK_QUERY = "QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5;";

export async function start(): Promise<void> {
	const workspace = query<HTMLElement>("[data-workspace]");
	const editorHost = byId("qql-editor");
	const editorLoading = query("[data-editor-loading]");

	const initialDoc = resolveInitialDocument(workspace);

	setEditor(
		createPlaygroundEditor({
			parent: editorHost,
			doc: initialDoc,
			diagnostics: editorDiagnostics,
			onDocChange: (source) => {
				setFixtureLabel("Custom query");
				queueAnalysis(source);
			},
			onCursor: followCursor,
			onRun: (shortcut) => {
				void (shortcut === "all" ? runAll() : runSmart());
			},
			onFormat: formatDocument,
			onSelectStatement: (index) => selectStatement(index, { reveal: true }),
		}),
	);

	wireChrome(workspace, setupPalette());
	await initializeRuntime(initialDoc, editorLoading);
}

/** `?q=` wins over the localStorage snapshot, which wins over the default. */
function resolveInitialDocument(workspace: HTMLElement | null): string {
	const params = new URLSearchParams(window.location.search);
	const urlQuery = params.get("q");
	const urlRef = params.get("ref") || params.get("from");

	if (urlQuery) {
		setFixtureLabel(
			urlRef?.includes("quickstart")
				? "Quickstart"
				: urlRef
					? "From docs"
					: "Shared query",
		);
	}
	const backlink = query<HTMLAnchorElement>("[data-docs-backlink]");
	if (backlink && urlRef) {
		backlink.href = urlRef;
		backlink.hidden = false;
		backlink.textContent = urlRef.includes("quickstart")
			? "Back to Quickstart →"
			: "Back to docs →";
	}

	let saved: string | null = null;
	try {
		saved = localStorage.getItem(WORKSPACE_KEY);
	} catch {
		// storage unavailable — fall through to the default query
	}
	return urlQuery ?? saved ?? workspace?.dataset.defaultQuery ?? FALLBACK_QUERY;
}

/** Attach every chrome surface. Runs once, before the runtime starts. */
function wireChrome(
	workspace: HTMLElement | null,
	palette: CommandPalette | null,
): void {
	applyPlatformKeyLabels();
	setupRunEngine();
	setupEditorActions();
	setupStatementNav();
	setupDocumentDelegates();

	setupSplitHandle(workspace);
	setupMobileViewSwitch(workspace);
	applyMobileView(workspace);
	applyWrap();

	setupInspectorTabs();
	setupDialog("preset-dialog", ["[data-open-presets]"]);
	setupDialog("settings-dialog", ["[data-open-settings]"]);
	setupDialog("policy-dialog", ["[data-open-policy]", "[data-policy-chip]"]);
	setupDialog("shortcuts-dialog", ["[data-open-shortcuts]"]);
	setupPresets();
	setupSettingsForm();
	setupPolicyForm();
	setupExporter();

	setupGlobalShortcuts({ palette });
	renderConnectionDisplay();
}

function setupPalette(): CommandPalette | null {
	const dialog = query<HTMLDialogElement>("#palette-dialog");
	if (!dialog) return null;
	return setupCommandPalette({ dialog, getCommands: buildCommands });
}

async function initializeRuntime(
	initialDoc: string,
	editorLoading: HTMLElement | null,
): Promise<void> {
	const runtimeDot = query("[data-runtime-dot]");
	const statusWasm = query("[data-status-wasm]");

	try {
		await initQql();
		setEmbedStatus(configureClient());

		runtimeDot?.classList.add("is-ready");
		if (statusWasm) statusWasm.textContent = "WASM Ready";
		editorLoading?.remove();
		setRunBusy(false);

		renderConnectionDisplay();
		state.statementSpans = scanStatementSpans(initialDoc);
		syncStatementState();
		runAnalysis(initialDoc);
		renderCursorStatus();
		void checkEndpoint();
	} catch (error) {
		const message = formatError(error);
		runtimeDot?.classList.add("is-failed");
		if (statusWasm) statusWasm.textContent = "WASM Failed";
		if (editorLoading) {
			editorLoading.textContent = `Failed to load WebAssembly: ${message}`;
		}
		showToast(message, true);
	}
}
