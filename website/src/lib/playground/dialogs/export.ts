/**
 * Export dialog: per-language SDK snippets generated from the live source and
 * the selected statement's compiled route.
 */
import {
	copyToClipboard,
	openDialog,
	query,
	queryAll,
	setupDialog,
} from "../core/dom";
import { sourceText, state, statementCount } from "../core/store";
import type { ExportLanguage } from "../core/types";
import { selectedRoute } from "../core/wasm";
import { exportCode } from "../services/codegen";

/** Regenerate the snippet for the current language + statement. */
function renderExport(): void {
	const output = query("[data-export-output]");
	if (!output) return;
	const count = Math.max(1, statementCount());
	const route = selectedRoute(state.analysis, state.selectedStatement);
	output.textContent = exportCode(
		state.exportLanguage,
		sourceText(),
		state.settings,
		route,
		state.selectedStatement,
		count,
	);

	const echo = query("[data-export-echo]");
	if (echo) {
		echo.textContent =
			state.exportLanguage === "curl"
				? `(cURL exports statement ${state.selectedStatement + 1} of ${count})`
				: count > 1
					? `(${count} statements in script)`
					: "";
	}

	queryAll<HTMLButtonElement>("[data-export-tab]").forEach((btn) => {
		btn.setAttribute(
			"aria-pressed",
			String(btn.dataset.exportTab === state.exportLanguage),
		);
	});
}

/** Render the current snippet, then open the dialog (menu + palette share this). */
export function openExporter(): void {
	renderExport();
	openDialog("export-dialog");
}

export function setupExporter(): void {
	const dialog = setupDialog("export-dialog", []);
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
