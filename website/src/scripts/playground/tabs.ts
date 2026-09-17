import { all, h, keepAnchored, required } from "./dom";
import type { InspectorTab } from "../playground-types";
import { INSPECTOR_TAB_KEY, saveSession } from "./store";

export const PRIMARY_TABS: InspectorTab[] = ["response", "plan", "wire"];
export const SECONDARY_TABS: InspectorTab[] = [
	"ast",
	"explain",
	"tokens",
	"metrics",
];
const TAB_LABELS: Record<InspectorTab, string> = {
	plan: "Plan",
	response: "Result",
	ast: "AST",
	wire: "Wire",
	explain: "Explain",
	tokens: "Tokens",
	metrics: "Metrics",
};

export function tabLabel(tab: InspectorTab): string {
	return TAB_LABELS[tab];
}

export interface TabsHooks {
	get: () => InspectorTab;
	set: (tab: InspectorTab) => void;
}

let activeHooks: TabsHooks | null = null;

function paint(): void {
	const hooks = activeHooks;
	if (!hooks) return;
	const current = hooks.get();
	for (const button of all<HTMLButtonElement>("[data-inspector-tab]")) {
		const active = button.dataset.inspectorTab === current;
		button.setAttribute("aria-selected", String(active));
		button.tabIndex = active ? 0 : -1;
	}
	const moreLabel = required<HTMLElement>("[data-more-label]");
	const secondary = SECONDARY_TABS.includes(current);
	moreLabel.textContent = secondary ? `More: ${tabLabel(current)}` : "More";
	for (const panel of all<HTMLElement>("[data-inspector-panel]")) {
		panel.hidden = panel.dataset.inspectorPanel !== current;
	}
}

/** Programmatic switch (run pipeline); repaints even before initTabs. */
export function switchTab(tab: InspectorTab, focus = false): void {
	if (activeHooks) {
		activeHooks.set(tab);
		saveSession(INSPECTOR_TAB_KEY, tab);
		paint();
	}
	if (focus) {
		document
			.querySelector<HTMLButtonElement>(`[data-inspector-tab="${tab}"]`)
			?.focus();
	}
	const more = document.querySelector<HTMLElement>("[data-more-menu]");
	if (more?.matches(":popover-open")) more.hidePopover();
}

/**
 * TabBar with 3 primary tabs + a More menu for compiler proof.
 * All seven buttons stay in the DOM (roles intact); the four secondary
 * ones live inside the More popover. Selecting one swaps the More label.
 */
export function initTabs(hooks: TabsHooks): void {
	activeHooks = hooks;
	const buttons = all<HTMLButtonElement>("[data-inspector-tab]");
	const more = required<HTMLElement>("[data-more-menu]");
	keepAnchored(more, required<HTMLElement>("[data-more-button]"));

	for (const button of buttons) {
		button.addEventListener("click", () => {
			switchTab(button.dataset.inspectorTab as InspectorTab);
		});
		button.addEventListener("keydown", (event) => {
			if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
			event.preventDefault();
			const order = buttons.filter((item) => item.offsetParent !== null);
			const pool = order.length > 0 ? order : buttons;
			const index = pool.indexOf(button);
			const direction = event.key === "ArrowRight" ? 1 : -1;
			const next = pool[(index + direction + pool.length) % pool.length];
			switchTab(next.dataset.inspectorTab as InspectorTab, true);
		});
	}

	more.addEventListener("toggle", () => {
		if (more.matches(":popover-open")) {
			more.querySelector<HTMLElement>("button")?.focus();
		}
	});

	paint();
}

/** Live count badge on the Result tab (`Result · 3`), cleared with null. */
export function setResultCount(count: number | null): void {
	const tab = document.querySelector<HTMLButtonElement>(
		'[data-inspector-tab="response"]',
	);
	if (!tab) return;
	tab.replaceChildren();
	tab.append(document.createTextNode("Result"));
	if (count != null) {
		tab.append(
			document.createTextNode(" · "),
			h(
				"span",
				{
					className:
						"rounded-full border border-[var(--sl-color-gray-5)] px-1 font-mono text-[0.625rem] tabular-nums",
				},
				String(count),
			),
		);
	}
}
