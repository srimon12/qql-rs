/**
 * Export dialog: per-language SDK snippets generated from the live source and
 * the selected statement's compiled route.
 */
import { copyToClipboard, openDialog, query, queryAll } from "./dom";
import { exportCode } from "./export";
import { sourceText, state, statementCount } from "./store";
import type { ExportLanguage } from "./types";
import { selectedRoute } from "./wasm";

/** Regenerate the snippet for the current language + statement. */
function renderExport(): void {
	const output = query("[data-export-output]");
	if (!output) return;
	output.textContent = exportCode(
		state.exportLanguage,
		sourceText(),
		state.settings,
		selectedRoute(state.analysis, state.selectedStatement),
		state.selectedStatement,
		Math.max(1, statementCount()),
	);
}

/** Render the current snippet, then open the dialog (menu + palette share this). */
export function openExporter(): void {
	renderExport();
	openDialog("export-dialog");
}

export function setupExporter(): void {
	const dialog = query<HTMLDialogElement>("#export-dialog");
	if (!dialog) return;

	const tabs = queryAll<HTMLButtonElement>("[data-export-tab]", dialog);
	for (const btn of tabs) {
		btn.addEventListener("click", () => {
			for (const other of tabs) other.setAttribute("aria-pressed", "false");
			btn.setAttribute("aria-pressed", "true");
			state.exportLanguage =
				(btn.dataset.exportTab as ExportLanguage) ?? "python";
			renderExport();
		});
	}

	query("[data-copy-export]", dialog)?.addEventListener("click", () => {
		void copyToClipboard(
			query("[data-export-output]", dialog)?.textContent ?? "",
			"Export code copied to clipboard",
		);
	});

	for (const btn of queryAll("[data-open-export]")) {
		btn.addEventListener("click", openExporter);
	}
}
