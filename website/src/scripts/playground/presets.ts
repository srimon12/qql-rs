import type { EditorView } from "@codemirror/view";
import { fetchCollectionTopology, listCollections } from "./client";
import { all, h, required } from "./dom";
import { replaceDoc } from "./editor";
import { connectionHost } from "./errors";
import type { LiveCollectionTopology } from "../playground-types";
import { store } from "./store";

function setupPresets(): void {
	const search = required<HTMLInputElement>("[data-preset-search]");
	const results = required<HTMLElement>("[data-preset-results]");
	const cards = all<HTMLButtonElement>("[data-preset]");
	const categories = all<HTMLButtonElement>("[data-preset-category]");
	let category = "all";

	const filter = () => {
		const query = search.value.trim().toLowerCase();
		for (const card of cards) {
			const categoryMatches =
				category === "all" || card.dataset.presetCategoryValue === category;
			const text = [
				card.dataset.presetLabel,
				card.dataset.presetDescription,
				card.dataset.presetId,
			]
				.join(" ")
				.toLowerCase();
			card.hidden = !categoryMatches || !text.includes(query);
		}
		const shown = cards.filter((card) => !card.hidden).length;
		results.textContent = `${shown} of ${cards.length} fixture examples shown`;
	};

	search.addEventListener("input", filter);
	for (const button of categories) {
		button.addEventListener("click", () => {
			category = button.dataset.presetCategory ?? "all";
			categories.forEach((item) => {
				item.setAttribute("aria-pressed", String(item === button));
			});
			filter();
		});
	}
	for (const card of cards) {
		card.addEventListener("click", () => {
			applyPreset(
				card.dataset.presetQuery ?? "",
				card.dataset.presetLabel ?? "Custom query",
			);
		});
		card.addEventListener("keydown", (event) => {
			const visible = cards.filter((item) => !item.hidden);
			const index = visible.indexOf(card);
			if (index === -1) return;
			let next: number | null = null;
			if (event.key === "Home") next = 0;
			if (event.key === "End") next = visible.length - 1;
			if (event.key === "ArrowRight" || event.key === "ArrowDown")
				next = index + 1;
			if (event.key === "ArrowLeft" || event.key === "ArrowUp")
				next = index - 1;
			if (next == null) return;
			event.preventDefault();
			visible[(next + visible.length) % visible.length]?.focus();
		});
	}
}
let boundEditor: EditorView | null = null;
let boundFixture: HTMLElement | null = null;

/** Bind once at boot, then wire the fixture browser. */
export function initPresets(
	editor: EditorView,
	activeFixture: HTMLElement,
): void {
	boundEditor = editor;
	boundFixture = activeFixture;
	setupPresets();
}

export function applyPreset(query: string, label: string): void {
	if (!boundEditor || !boundFixture) return;
	replaceDoc(boundEditor, query);
	boundFixture.textContent = label;
	required<HTMLDialogElement>("#preset-dialog").close();
	boundEditor.focus();
}

/** One-click presets derived from live GET /collections, USING prefilled. */ export async function loadLivePresets(): Promise<void> {
	const list = required<HTMLElement>("[data-live-presets]");
	const hint = required<HTMLElement>("[data-live-hint]");
	list.replaceChildren();
	try {
		const base = store.settings.qdrantUrl.replace(/\/+$/, "");
		const names = await listCollections(base, store.settings.qdrantKey);
		if (names.length === 0) {
			hint.textContent = "No collections on this Qdrant yet.";
			return;
		}
		const topologies = await Promise.all(
			names.slice(0, 6).map((name) =>
				fetchCollectionTopology(
					store.settings.qdrantUrl.replace(/\/+$/, ""),
					store.settings.qdrantKey,
					name,
				).catch(
					(): LiveCollectionTopology => ({
						name,
						dense: [],
						sparse: [],
						multi: [],
					}),
				),
			),
		);
		let shown = 0;
		for (const topology of topologies) {
			if (shown >= 4) break;
			const card = buildLivePresetCard(topology);
			if (card) {
				list.append(card);
				shown += 1;
			}
		}
		hint.textContent = `${names.length} ${names.length === 1 ? "collection" : "collections"} on ${connectionHost(base)} — presets prefill USING.`;
	} catch {
		hint.textContent =
			"Qdrant unreachable — live presets need a running Qdrant.";
	}
}
function buildLivePresetCard(
	topology: LiveCollectionTopology,
): HTMLButtonElement | null {
	let label: string;
	let description: string;
	let query: string;
	if (topology.dense.length > 0) {
		const vector = topology.dense.includes("dense")
			? "dense"
			: topology.dense[0];
		label = `${topology.name} — text search`;
		description = `TEXT query with USING ${vector} AS DENSE prefilled.`;
		query = `QUERY 'example search' FROM ${topology.name} USING ${vector} AS DENSE LIMIT 5;`;
	} else {
		label = `${topology.name} — count`;
		description = "No dense vector here; COUNT needs no USING.";
		query = `COUNT FROM ${topology.name};`;
	}
	return h(
		"button",
		{
			type: "button",
			className:
				"grid content-start gap-1 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-3 text-left text-[var(--sl-color-gray-2)] transition-colors hover:border-[var(--sl-color-gray-4)] hover:bg-[var(--sl-color-bg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--sl-color-accent)]",
			dataset: { livePreset: topology.name },
			onClick: () => applyPreset(query, label),
		},
		h(
			"span",
			{
				className:
					"font-mono text-[0.625rem] uppercase tracking-[0.08em] text-[var(--sl-color-gray-3)]",
			},
			"Live",
		),
		h(
			"strong",
			{
				className:
					"text-[0.8125rem] font-semibold text-[var(--sl-color-white)]",
			},
			label,
		),
		h(
			"span",
			{
				className:
					"line-clamp-2 text-xs leading-5 text-[var(--sl-color-gray-3)]",
			},
			description,
		),
	);
}
