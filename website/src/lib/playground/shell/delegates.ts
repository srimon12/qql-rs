/**
 * Document-level click delegation for markup rendered by `render.ts` and the
 * inspector: copy buttons, "load into editor" buttons, and raw-tab copies.
 * Statement chip / route-row clicks are owned by `selection.ts`.
 */

import { copyToClipboard, query, showToast } from "../core/dom";
import { loadSource } from "../editor/analysis";

export function setupDocumentDelegates(): void {
	document.addEventListener("click", (event) => {
		const target = event.target as HTMLElement;

		const copyJson = target.closest<HTMLElement>("[data-copy-json]");
		if (copyJson?.dataset.copyJson) {
			void copyToClipboard(
				copyJson.dataset.copyJson,
				"Copied JSON to clipboard",
			);
			return;
		}

		const copyText = target.closest<HTMLElement>("[data-copy-text]");
		if (copyText?.dataset.copyText) {
			void copyToClipboard(copyText.dataset.copyText, "Copied ID to clipboard");
			return;
		}

		const loadQuery = target.closest<HTMLElement>("[data-load-query]");
		if (loadQuery?.dataset.loadQuery) {
			loadSource(loadQuery.dataset.loadQuery);
			showToast("Loaded query into editor");
			return;
		}

		const copyTab = target.closest<HTMLElement>("[data-copy-tab]");
		if (copyTab?.dataset.copyTab) {
			const pre = query(`[data-output="${copyTab.dataset.copyTab}"]`);
			if (pre?.textContent) {
				void copyToClipboard(
					pre.textContent,
					`Copied ${copyTab.dataset.copyTab} to clipboard`,
				);
			}
		}
	});
}
