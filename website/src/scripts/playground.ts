import initQql from "qql-wasm-current";
import { currentDiagnostic } from "./playground/analysis";
import { pool } from "./playground/client";
import {
	initDialogs,
	initDocsBacklink,
	initOverflow,
	initShare,
	openDialog,
} from "./playground/dialogs";
import { required } from "./playground/dom";
import {
	createEditor,
	markCustomQuery,
	persistWorkspace,
	resolveInitialDoc,
	setupWrapToggle,
} from "./playground/editor";
import { formatError } from "./playground/errors";
import { configureClient, initForms } from "./playground/forms";
import { checkEndpoint, initHealth } from "./playground/health";
import { initInspector, renderOutputs } from "./playground/inspector";
import {
	onStatementChange,
	selectStatement,
	statementValue,
} from "./playground/plan";
import { initPresets, loadLivePresets } from "./playground/presets";
import {
	applyUsing,
	executeQuery,
	formatEditor,
	initRun,
	queueAnalysis,
	runAnalysis,
} from "./playground/run";
import {
	applyPlatformKeyHints,
	initStatusBar,
	setEditorLoading,
	setFirstRunPing,
	setRuntime,
} from "./playground/statusbar";
import { hasRunOnce, SPLIT_KEY, store } from "./playground/store";
import { initTabs } from "./playground/tabs";
import { initToasts, toast } from "./playground/toasts";

const workspace = required<HTMLElement>("[data-default-query]");
const editorHost = required<HTMLElement>("#qql-editor");
const runButton = required<HTMLButtonElement>("[data-run]");
const formatButton = required<HTMLButtonElement>("[data-format]");
const validationBadge = required<HTMLElement>("[data-validation-badge]");
const analysisSummary = required<HTMLElement>("[data-analysis-summary]");
const runtimeStatus = required<HTMLElement>("[data-runtime-status]");
const runtimeDot = required<HTMLElement>("[data-runtime-dot]");
const connectionValue = required<HTMLElement>("[data-connection-value]");
const activeFixture = required<HTMLElement>("[data-active-fixture]");
const policyDot = required<HTMLElement>("[data-policy-dot]");
const policyChip = required<HTMLButtonElement>("[data-policy-chip]");
const runLabel = required<HTMLElement>("[data-run-label]");
const toastRegion = required<HTMLElement>("[data-toast-region]");
const responseRich = required<HTMLElement>("[data-response-rich]");
const responsePre = required<HTMLElement>(`[data-output="response"]`);
const editorLoading = required<HTMLElement>("[data-editor-loading]");
const statusEmbed = required<HTMLElement>("[data-status-embed]");
const statusWasm = required<HTMLElement>("[data-status-wasm]");

const state = store.state;

const editor = createEditor(
	editorHost,
	resolveInitialDoc(workspace.dataset.defaultQuery ?? ""),
	{
		getDiagnostics: () => currentDiagnostic(state.analysis),
		onChange: (source) => {
			markCustomQuery(activeFixture);
			persistWorkspace(source);
			queueAnalysis(source);
		},
		onRun: () => void executeQuery(),
		onOpenPresets: () => openDialog("#preset-dialog"),
	},
);

function setupSplit(): void {
	const handle = document.querySelector<HTMLElement>("[data-split-handle]");
	if (!handle) return;
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
	initToasts(toastRegion);
	initStatusBar({
		runtimeStatus,
		runtimeDot,
		connectionValue,
		validationBadge,
		statusEmbed,
		policyDot,
		policyChip,
		editorHost,
		editorLoading,
	});
	applyPlatformKeyHints();
	setupSplit();
	setupWrapToggle(
		editor,
		editorHost,
		required<HTMLButtonElement>("[data-wrap-toggle]"),
	);
	initTabs({
		get: () => state.inspectorTab,
		set: (tab) => {
			state.inspectorTab = tab;
		},
	});
	initDialogs();
	initOverflow();
	initPresets(editor, activeFixture);
	initShare(() => editor.state.doc.toString());
	initDocsBacklink(
		new URLSearchParams(window.location.search).get("from") ??
			new URLSearchParams(window.location.search).get("ref"),
	);
	initForms(editor, { runAnalysis, checkEndpoint });
	initRun(editor, activeFixture);
	initInspector({ executeQuery, applyUsing });
	initHealth({ renderOutputs });
	onStatementChange(() => selectStatement(statementValue()));
	runButton.addEventListener("click", () => void executeQuery());
	formatButton.addEventListener("click", formatEditor);

	try {
		await initQql();
		configureClient();
		setRuntime("Current qql-rs WASM ready", "ready");
		statusWasm.textContent = "Ready";
		setFirstRunPing(!hasRunOnce());
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
	pool.free();
});

void start();
