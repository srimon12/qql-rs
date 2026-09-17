import {
	displayJson,
	formatId,
	formatScalar,
	highlightJson,
	pretty,
} from "./display";
import { h } from "./dom";
import {
	copyBtn,
	scoreBar,
	scorePill,
} from "./badges";

export interface ResultHit {
	id: string | number | bigint;
	score?: number;
	payload?: Record<string, unknown> | null;
}

export function isHitArray(data: unknown): data is ResultHit[] {
	return (
		Array.isArray(data) &&
		data.length > 0 &&
		typeof data[0] === "object" &&
		data[0] !== null &&
		"id" in data[0]
	);
}

export function isFacetArray(
	data: unknown,
): data is Array<{ value: unknown; count: number }> {
	return (
		Array.isArray(data) &&
		data.every(
			(entry) =>
				entry &&
				typeof entry === "object" &&
				"value" in entry &&
				"count" in entry,
		)
	);
}

function payloadRank(key: string, value: unknown): number {
	if (key === "name") return 0;
	if (value == null || typeof value === "number" || typeof value === "boolean")
		return 1;
	if (typeof value === "string") return value.length < 40 ? 1 : 2;
	return 3;
}

function payloadRow(key: string, value: unknown): HTMLElement {
	const term = h(
		"dt",
		{
			className:
				"m-0 min-w-0 truncate font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
			title: key,
		},
		key,
	);
	const definition = h(
		"dd",
		{
			className:
				"m-0 min-w-0 break-words font-mono text-[0.75rem] leading-6 text-[var(--sl-color-gray-1)]",
		},
		formatScalar(value),
	);
	if (
		key === "description" &&
		typeof value === "string" &&
		value.length > 120
	) {
		definition.classList.add("line-clamp-2");
		const more = h(
			"button",
			{ type: "button", className: "link font-mono text-[0.6875rem]" },
			"Show more",
		);
		more.addEventListener("click", () => {
			const open = definition.classList.toggle("line-clamp-2");
			more.textContent = open ? "Show more" : "Show less";
		});
		return h(
			"div",
			{
				className:
					"grid gap-1 px-3 py-2 sm:grid-cols-[40%_minmax(0,1fr)] sm:gap-2",
			},
			term,
			h(
				"div",
				{ className: "grid min-w-0 content-start gap-1" },
				definition,
				more,
			),
		);
	}
	return h(
		"div",
		{
			className:
				"grid gap-0.5 px-3 py-2 sm:grid-cols-[40%_minmax(0,1fr)] sm:gap-2",
		},
		term,
		definition,
	);
}

function payloadTable(payload: Record<string, unknown>): HTMLElement {
	const ordered = Object.entries(payload).sort(
		([ak, av], [bk, bv]) => payloadRank(ak, av) - payloadRank(bk, bv),
	);
	const list = h("dl", {
		className:
			"m-0 grid divide-y divide-[var(--sl-color-gray-5)] rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)]",
	});
	const VISIBLE = 6;
	const render = (count: number): void => {
		list.replaceChildren();
		for (const [key, value] of ordered.slice(0, count))
			list.append(payloadRow(key, value));
		if (ordered.length > count) {
			const more = h(
				"button",
				{
					type: "button",
					className:
						"flex w-full items-center justify-between px-3 py-2 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] transition-colors hover:text-[var(--sl-color-gray-1)]",
					ariaExpanded: "false",
				},
				`+${ordered.length - count} more fields`,
				h("span", { ariaHidden: "true" }, "▾"),
			);
			more.addEventListener("click", () => render(ordered.length));
			list.append(more);
		}
	};
	render(Math.min(VISIBLE, ordered.length));
	return list;
}

/** One scored hit readable in 3 seconds: rank + id, score, hero, payload. */
export function hitCard(
	hit: ResultHit,
	rank: number,
	barFraction: number,
): HTMLElement {
	const idText = formatId(hit.id);
	const score = typeof hit.score === "number" ? hit.score : null;
	const outOfRange = score != null && (score < 0 || score > 1);
	const idBtn = h(
		"button",
		{
			type: "button",
			className:
				"min-w-0 truncate text-left font-mono text-[0.75rem] font-semibold text-[var(--sl-color-accent-high)] hover:underline",
			title: `${idText} (click to copy)`,
		},
		`#${rank} · #${idText}`,
	);
	idBtn.addEventListener("click", () => {
		void navigator.clipboard?.writeText(idText).catch(() => undefined);
	});
	const header = h(
		"header",
		{ className: "flex min-w-0 items-center justify-between gap-2 px-3 pt-3" },
		idBtn,
		score != null ? scorePill(score, outOfRange) : h("span", null),
	);
	const card = h(
		"article",
		{
			className:
				"hit-card grid content-start gap-2 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] pb-3",
		},
		header,
		score != null
			? h(
					"div",
					{ className: "px-3" },
					scoreBar(barFraction, `score ${score.toFixed(4)}`),
				)
			: null,
	);
	const payload =
		hit.payload && typeof hit.payload === "object" ? hit.payload : null;
	const name =
		payload && typeof payload.name === "string" ? payload.name : null;
	if (name) {
		card.append(
			h(
				"h3",
				{
					className:
						"m-0 px-3 font-(family-name:--sl-font-serif) text-[1.0625rem] font-light leading-snug tracking-[-0.01em] text-[var(--sl-color-white)]",
				},
				name,
			),
		);
		const sub: string[] = [];
		if (payload && typeof payload.rating === "number")
			sub.push(`★ ${payload.rating}`);
		if (
			payload &&
			(typeof payload.price === "number" || typeof payload.price === "string")
		)
			sub.push(`$${payload.price}`);
		if (payload && typeof payload.neighbourhood === "string")
			sub.push(payload.neighbourhood);
		if (sub.length > 0)
			card.append(
				h(
					"p",
					{
						className:
							"m-0 truncate px-3 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
					},
					sub.join(" · "),
				),
			);
	}
	if (payload)
		card.append(h("div", { className: "px-3" }, payloadTable(payload)));
	card.append(
		h(
			"footer",
			{
				className:
					"flex items-center justify-between gap-2 border-t border-[var(--sl-color-gray-5)] px-3 pt-2",
			},
			copyBtn("Copy JSON", () => pretty(hit)),
			h(
				"button",
				{
					type: "button",
					className: "app-action min-h-6 px-1.5",
					ariaExpanded: "false",
				},
				"Raw",
			),
		),
	);
	const rawBtn = card.querySelector("footer > button.app-action");
	const rawPre = h("pre", {
		className:
			"pg-hl m-0 mx-3 hidden overflow-auto rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem] leading-5 whitespace-pre-wrap break-words",
		html: highlightJson(displayJson(hit)),
	});
	rawBtn?.addEventListener("click", () => {
		const open = rawPre.classList.toggle("hidden");
		rawBtn.setAttribute("aria-expanded", String(!open));
	});
	card.append(rawPre);
	return card;
}

export function statementTag(operation: string, ok: boolean): HTMLElement {
	return h(
		"p",
		{ className: "m-0 flex items-center gap-2 font-mono text-[0.6875rem]" },
		h(
			"span",
			{
				className:
					"rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-7)] px-1.5 py-px text-[0.625rem] font-bold text-[var(--q-sx-keyword)]",
			},
			operation,
		),
		h(
			"span",
			{ className: ok ? "text-[var(--q-ok)]" : "text-[var(--q-bad)]" },
			ok ? "ok" : "failed",
		),
	);
}
