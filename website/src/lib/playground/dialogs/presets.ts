/**
 * Examples dialog: search + category filtering for the shipped fixture
 * corpus, plus the live-collection chips fetched from the user's Qdrant.
 */

import { byId, escapeHtml, query, queryAll, showToast } from "../core/dom";
import { loadSource } from "../editor/analysis";

let activeCategory = "all";

function filterPresets(): void {
	const dialog = byId<HTMLDialogElement>("preset-dialog");
	const term = (
		query<HTMLInputElement>("[data-preset-search]", dialog)?.value ?? ""
	)
		.toLowerCase()
		.trim();
	queryAll<HTMLElement>("[data-preset]", dialog).forEach((item) => {
		const category = item.dataset.presetCategoryValue || "";
		const categoryMatch =
			activeCategory === "all" || category === activeCategory;
		const termMatch =
			!term || (item.textContent || "").toLowerCase().includes(term);
		item.hidden = !(categoryMatch && termMatch);
	});
}

/** Live collections from the current endpoint, as one-click SCROLL presets. */
export function renderLiveCollections(collections: string[]): void {
	const container = query("[data-live-presets]");
	if (!container) return;
	const hint = query("[data-live-hint]");
	if (collections.length === 0) return;
	if (hint) {
		hint.textContent = `Found ${collections.length} collection(s) on your endpoint:`;
	}
	container.innerHTML = collections
		.map(
			(name) => `
				<button type="button" class="app-chip text-left justify-start" data-preset-query="SCROLL FROM ${escapeHtml(name)} LIMIT 5;" data-preset-label="${escapeHtml(name)}">
					<span class="app-chip__label">Collection</span>
					<span class="app-chip__value font-mono font-bold">${escapeHtml(name)}</span>
				</button>`,
		)
		.join("");
}

export function setupPresets(): void {
	const dialog = byId<HTMLDialogElement>("preset-dialog");
	const search = query<HTMLInputElement>("[data-preset-search]", dialog);
	const categoryButtons = queryAll<HTMLButtonElement>(
		"[data-preset-category]",
		dialog,
	);

	search?.addEventListener("input", filterPresets);
	categoryButtons.forEach((btn) => {
		btn.addEventListener("click", () => {
			for (const other of categoryButtons) {
				other.setAttribute("aria-pressed", "false");
			}
			btn.setAttribute("aria-pressed", "true");
			activeCategory = btn.dataset.presetCategory || "all";
			filterPresets();
		});
	});

	// One delegated handler serves both fixture cards and live-collection
	// chips, including ones rendered after this setup runs.
	document.addEventListener("click", (event) => {
		const item = (event.target as HTMLElement).closest<HTMLElement>(
			"[data-preset-query]",
		);
		if (!item) return;
		const source = item.dataset.presetQuery;
		if (!source) return;
		const label = item.dataset.presetLabel || "Example";
		loadSource(source, label);
		dialog.close();
		showToast(`Loaded example: ${label}`);
	});
}
