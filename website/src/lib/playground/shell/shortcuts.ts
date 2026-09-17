/**
 * Global keyboard shortcuts that work outside the editor: ⌘K for the palette,
 * ⌘↵ / ⇧⌘↵ to run, Escape to close the run menu. Editor-focused keys live in
 * `editor.ts`'s keymap.
 */
import { queryAll } from "../core/dom";
import { editor } from "../core/store";
import { closeRunMenu, runAll, runSmart } from "../run/run";
import type { CommandPalette } from "./palette";

export interface ShortcutOptions {
	palette: CommandPalette | null;
}

export function setupGlobalShortcuts({ palette }: ShortcutOptions): void {
	queryAll("[data-open-palette]").forEach((btn) => {
		btn.addEventListener("click", () => palette?.open());
	});

	document.addEventListener("keydown", (event) => {
		if (event.key === "Escape") {
			closeRunMenu();
			return;
		}
		if (!(event.metaKey || event.ctrlKey)) return;
		// While a dialog is open only the palette itself keeps its shortcuts.
		if (document.querySelector("dialog[open]") && !palette?.isOpen) return;

		if (event.key.toLowerCase() === "k") {
			event.preventDefault();
			palette?.toggle();
			return;
		}
		if (event.key === "Enter" && !editor().view.hasFocus) {
			event.preventDefault();
			void (event.shiftKey ? runAll() : runSmart());
		}
	});
}
