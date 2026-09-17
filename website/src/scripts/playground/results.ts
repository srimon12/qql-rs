import { displayJson, highlightJson } from "./display";
import { h } from "./dom";
import {
	hitCard,
	isFacetArray,
	isHitArray,
	type ResultHit,
	statementTag,
} from "./hits";
import { statBadge } from "./badges";
import { countHero, facetList, groupCard } from "./shapes";

interface ReportResult {
	ok: boolean;
	operation: string;
	message: string;
	data: unknown;
}

function resultSection(
	result: ReportResult,
	multi: boolean,
	ms: string,
	headline: boolean,
): HTMLElement {
	const section = h("section", { className: "grid content-start gap-3" });
	const data = result.data;
	if (!result.ok) {
		section.append(
			statementTag(result.operation, false),
			h(
				"p",
				{ className: "m-0 text-[0.8125rem] text-[var(--sl-color-gray-1)]" },
				result.message || "Statement failed.",
			),
		);
		return section;
	}
	if (isHitArray(data)) {
		const scores = data
			.map((hit) => (typeof hit.score === "number" ? hit.score : null))
			.filter((score): score is number => score != null);
		const min = Math.min(...scores, 0);
		const max = Math.max(...scores, 1);
		const span = max - min || 1;
		if (headline)
			section.append(
				statBadge(`${data.length} ${data.length === 1 ? "hit" : "hits"}${ms}`),
			);
		if (multi) section.append(statementTag(result.operation, true));
		const animated = data.slice(0, 30);
		animated.forEach((hit, hitIndex) => {
			const fraction =
				typeof hit.score === "number" ? (hit.score - min) / span : 1;
			const card = hitCard(hit, hitIndex + 1, fraction);
			if (hitIndex < 8) {
				card.classList.add("hit-enter");
				card.style.animationDelay = `${hitIndex * 40}ms`;
			}
			section.append(card);
		});
		if (data.length > 30)
			section.append(
				h(
					"p",
					{
						className:
							"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
					},
					`Showing 30 of ${data.length} — see Raw JSON for the rest.`,
				),
			);
		return section;
	}
	if (
		result.operation === "COUNT" &&
		data &&
		typeof data === "object" &&
		!Array.isArray(data) &&
		typeof (data as { count?: unknown }).count === "number"
	) {
		section.append(countHero((data as { count: number }).count, ms));
		if (multi) section.append(statementTag(result.operation, true));
		return section;
	}
	if (isFacetArray(data)) {
		if (headline) section.append(statBadge(`${data.length} facet values${ms}`));
		if (multi) section.append(statementTag(result.operation, true));
		section.append(facetList(data));
		return section;
	}
	if (
		data &&
		typeof data === "object" &&
		!Array.isArray(data) &&
		Array.isArray((data as { groups?: unknown }).groups)
	) {
		const groups = (
			data as { groups: Array<{ id: unknown; hits: ResultHit[] }> }
		).groups;
		if (headline) section.append(statBadge(`${groups.length} groups${ms}`));
		if (multi) section.append(statementTag(result.operation, true));
		for (const group of groups.slice(0, 10)) section.append(groupCard(group));
		return section;
	}
	const mutCount =
		data &&
		typeof data === "object" &&
		!Array.isArray(data) &&
		typeof (data as { count?: unknown }).count === "number"
			? (data as { count: number }).count
			: null;
	section.append(
		h(
			"div",
			{
				className:
					"flex items-center gap-2 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3",
			},
			h("span", {
				className: "size-1.5 shrink-0 rounded-full bg-[var(--q-ok)]",
				ariaHidden: "true",
			}),
			h(
				"p",
				{ className: "m-0 text-[0.8125rem] text-[var(--sl-color-gray-1)]" },
				result.message ||
					(mutCount != null
						? `${result.operation} · ${mutCount} points${ms}`
						: `${result.operation} ok${ms}`),
			),
		),
	);
	if (multi) section.append(statementTag(result.operation, true));
	return section;
}

/** Hit count for the Result badge + foot flip; null when not a hit set. */
export function resultHitCount(report: {
	ok: boolean;
	results: Array<{ ok: boolean; data: unknown }>;
}): number | null {
	if (report.results.length !== 1) return null;
	const data = report.results[0]?.data;
	return report.results[0]?.ok && isHitArray(data) ? data.length : null;
}

export interface ReportBranch {
	filterLabel: string;
	filterQuery: string;
	onLoadFilter: () => void;
	onSeeWire: () => void;
	onExportPython: () => void;
}

/** Full rich Result body: outcome line, per-statement sections, Raw JSON. */
export function renderReport(
	report: {
		ok: boolean;
		results: ReportResult[];
		succeeded: number;
		failed: number;
	},
	executeMs: number | null,
	branch?: ReportBranch,
): HTMLElement {
	const ms = executeMs != null ? ` · ${executeMs.toFixed(0)} ms` : "";
	const root = h("div", { className: "grid content-start gap-3" });
	const multi = report.results.length > 1;
	if (!multi) {
		const first = report.results[0];
		const headline =
			first?.ok && isHitArray(first.data)
				? `${first.data.length} ${first.data.length === 1 ? "hit" : "hits"}${ms}`
				: report.ok
					? `${report.succeeded} succeeded${ms}`
					: `${report.succeeded} succeeded · ${report.failed} failed${ms}`;
		root.append(statBadge(headline, report.ok));
	}
	report.results.forEach((result, index) => {
		if (multi) root.append(statBadge(`#${index + 1}${ms}`, result.ok));
		root.append(resultSection(result, multi, ms, multi));
	});
	if (
		branch &&
		!multi &&
		report.ok &&
		report.results[0]?.ok &&
		isHitArray(report.results[0]?.data)
	) {
		const cards: Array<[string, string, () => void]> = [
			["Try a filter", branch.filterLabel, branch.onLoadFilter],
			["See the wire", "Compiled REST payload", branch.onSeeWire],
			["Export Python", "SDK code for this query", branch.onExportPython],
		];
		const row = h("div", {
			className: "grid gap-2 sm:grid-cols-3",
		});
		for (const [title, hint, run] of cards) {
			const card = h(
				"button",
				{
					type: "button",
					className:
						"grid content-start gap-1 rounded-[var(--q-r-sm)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-3 text-left transition-colors hover:border-[var(--sl-color-gray-4)] hover:bg-[var(--sl-color-bg)]",
				},
				h(
					"strong",
					{
						className:
							"text-[0.8125rem] font-semibold text-[var(--sl-color-white)]",
					},
					title,
				),
				h(
					"span",
					{ className: "text-xs leading-5 text-[var(--sl-color-gray-3)]" },
					hint,
				),
			);
			card.addEventListener("click", run);
			row.append(card);
		}
		root.append(row);
	}
	const details = h(
		"details",
		{ className: "pg-details grid gap-2" },
		h(
			"summary",
			{
				className:
					"cursor-pointer font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]",
			},
			"Raw JSON",
		),
		h("pre", {
			className:
				"pg-hl m-0 overflow-auto rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem] leading-5 whitespace-pre-wrap break-words text-[var(--sl-color-gray-2)]",
			html: highlightJson(displayJson(report)),
		}),
	);
	root.append(details);
	return root;
}
