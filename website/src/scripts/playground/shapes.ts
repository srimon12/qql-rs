import { formatId, formatScalar } from "./display";
import { h } from "./dom";
import type { ResultHit } from "./hits";

export function facetList(
	rows: Array<{ value: unknown; count: number }>,
): HTMLElement {
	const max = Math.max(1, ...rows.map((row) => row.count));
	const list = h("div", {
		className:
			"grid gap-1 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3",
	});
	rows.slice(0, 20).forEach((row, index) => {
		const label = formatScalar(row.value);
		const valueBtn = h(
			"button",
			{
				type: "button",
				className:
					"min-w-0 truncate text-left font-mono text-[0.75rem] text-[var(--sl-color-gray-1)] hover:underline",
				title: `${label} (click to copy)`,
			},
			label,
		);
		valueBtn.addEventListener("click", () => {
			void navigator.clipboard?.writeText(label).catch(() => undefined);
		});
		const fill = h("span", {
			className: `block h-full rounded-full ${index < 3 ? "bg-[var(--sl-color-accent)]" : "bg-[var(--sl-color-gray-4)]"}`,
			role: "img",
			ariaLabel: `${label}: ${row.count}`,
		});
		fill.style.width = `${Math.max(4, Math.round((row.count / max) * 100))}%`;
		list.append(
			h(
				"div",
				{ className: "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2" },
				valueBtn,
				h(
					"span",
					{
						className:
							"font-mono text-[0.6875rem] tabular-nums text-[var(--sl-color-gray-3)]",
					},
					String(row.count),
				),
			),
			h(
				"div",
				{
					className:
						"h-[3px] overflow-hidden rounded-full bg-[var(--sl-color-gray-5)]",
					ariaHidden: "true",
				},
				fill,
			),
		);
	});
	return list;
}

export function groupCard(group: {
	id: unknown;
	hits: ResultHit[];
}): HTMLElement {
	const hits = group.hits ?? [];
	const block = h(
		"div",
		{
			className:
				"grid content-start gap-1.5 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3",
		},
		h(
			"p",
			{
				className:
					"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
			},
			"group ",
			h(
				"span",
				{ className: "text-[var(--sl-color-accent-high)]" },
				formatScalar(group.id),
			),
			` · ${hits.length} hits`,
		),
	);
	const renderNested = (count: number): void => {
		for (const node of block.querySelectorAll("[data-nested-hit]"))
			node.remove();
		block.querySelector("[data-show-all]")?.remove();
		for (const hit of hits.slice(0, count)) {
			block.append(
				h(
					"p",
					{
						className:
							"m-0 flex justify-between gap-2 border-l-2 border-[var(--sl-color-accent)] pl-2 font-mono text-[0.6875rem]",
						dataset: { nestedHit: "" },
					},
					h(
						"span",
						{
							className: "min-w-0 truncate text-[var(--sl-color-accent-high)]",
						},
						`#${formatId(hit.id)}`,
					),
					typeof hit.score === "number"
						? h(
								"span",
								{
									className:
										"shrink-0 tabular-nums text-[var(--sl-color-gray-3)]",
								},
								hit.score.toFixed(4),
							)
						: null,
				),
			);
		}
		if (hits.length > count) {
			const show = h(
				"button",
				{
					type: "button",
					className: "app-action min-h-6 w-fit px-1.5",
					dataset: { showAll: "" },
				},
				`Show all ${hits.length}`,
			);
			show.addEventListener("click", () => renderNested(hits.length));
			block.append(show);
		}
	};
	renderNested(Math.min(2, hits.length));
	return block;
}

export function countHero(count: number, ms: string): HTMLElement {
	return h(
		"div",
		{
			className:
				"grid content-center justify-items-center gap-1 px-4 py-12 text-center",
		},
		h(
			"p",
			{
				className:
					"m-0 font-(family-name:--sl-font-serif) text-[2rem] font-light tabular-nums text-[var(--sl-color-white)]",
			},
			count.toLocaleString("en-US"),
		),
		h(
			"p",
			{
				className:
					"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
			},
			`points match${ms}`,
		),
	);
}
