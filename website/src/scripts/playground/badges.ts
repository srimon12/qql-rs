import { attachCopy, h } from "./dom";

export function copyBtn(label: string, getText: () => string): HTMLButtonElement {
	const btn = h(
		"button",
		{
			type: "button",
			className: "copy-btn copy-btn--reveal",
			ariaLabel: `${label} (copy)`,
		},
		h(
			"span",
			{ className: "copy-btn__label", dataset: { copyLabel: "" } },
			label,
		),
	);
	attachCopy(btn, getText);
	return btn;
}

/** One-line outcome summary above every result set. */
export function statBadge(text: string, ok = true): HTMLElement {
	return h(
		"p",
		{
			className:
				"m-0 flex items-center gap-1.5 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
		},
		h("span", {
			className: `size-1.5 shrink-0 rounded-full ${ok ? "bg-[var(--q-ok)]" : "bg-[var(--q-bad)]"}`,
			ariaHidden: "true",
		}),
		h("span", { className: "tabular-nums", text }),
	);
}

export function scorePill(score: number, outOfRange: boolean): HTMLElement {
	const pill = h(
		"span",
		{
			className:
				"shrink-0 rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-2 py-0.5 font-mono text-[0.6875rem] tabular-nums text-[var(--sl-color-gray-2)]",
		},
		`score ${score.toFixed(4)}`,
	);
	if (outOfRange)
		pill.setAttribute(
			"title",
			`Raw score ${score} is outside [0, 1]; bar clamped.`,
		);
	return pill;
}

export function scoreBar(fraction: number, label: string): HTMLElement {
	const fill = h("span", {
		className:
			"scorebar-fill block h-full rounded-full bg-[var(--sl-color-accent)]",
		role: "img",
		ariaLabel: label,
	});
	fill.style.width = "0%";
	requestAnimationFrame(() => {
		fill.style.width = `${Math.max(2, Math.min(100, Math.round(fraction * 100)))}%`;
	});
	return h(
		"div",
		{
			className:
				"h-[3px] overflow-hidden rounded-full bg-[var(--sl-color-gray-5)]",
			ariaHidden: "true",
		},
		fill,
	);
}

