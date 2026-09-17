import { highlightJson, prettyRaw } from "./display";
import { h } from "./dom";
import type {
	LiveCollectionTopology,
	PlaygroundFailure,
} from "../playground-types";

export interface ErrorHooks {
	resolveTopology: (collection: string) => Promise<LiveCollectionTopology>;
	onApplyUsing: (collection: string, vector: string) => void;
	onLoadQuery: (query: string, label: string) => void;
	onOpenSettings: () => void;
}

function fixSuggestion(
	failure: PlaygroundFailure,
	hooks: ErrorHooks,
): HTMLElement | null {
	const loadPill = (label: string, query: string): HTMLElement => {
		const pill = h(
			"button",
			{ type: "button", className: "app-action app-action--lg w-fit" },
			label,
		);
		pill.addEventListener("click", () => hooks.onLoadQuery(query, label));
		return pill;
	};
	const wrap = (text: string, action: HTMLElement | null): HTMLElement => {
		const box = h(
			"div",
			{ className: "grid content-start gap-2" },
			h(
				"p",
				{ className: "m-0 text-xs leading-5 text-[var(--sl-color-gray-2)]" },
				text,
			),
		);
		if (action) box.append(action);
		return box;
	};
	switch (failure.code) {
		case "QQL-BACKEND-COLLECTION-NOT-FOUND":
			return wrap(
				"Try SHOW COLLECTIONS to list live collections.",
				loadPill("Load SHOW COLLECTIONS", "SHOW COLLECTIONS;"),
			);
		case "QQL-BACKEND-DIMENSION-MISMATCH":
			return wrap(
				"Use a TEXT query with USING, or match the collection dimension.",
				null,
			);
		default:
			if (failure.code?.startsWith("QQL-TRANSPORT")) {
				const open = h(
					"button",
					{ type: "button", className: "app-action app-action--lg w-fit" },
					"Open connection",
				);
				open.addEventListener("click", hooks.onOpenSettings);
				return wrap(
					"Start Qdrant locally with the default REST port 6333, then reload.",
					open,
				);
			}
			if (failure.code?.startsWith("QQL-EMBEDDING")) {
				const open = h(
					"button",
					{ type: "button", className: "app-action app-action--lg w-fit" },
					"Open connection",
				);
				open.addEventListener("click", hooks.onOpenSettings);
				return wrap(
					"Check the endpoint, model, and dimension in Connection settings.",
					open,
				);
			}
			return null;
	}
}

/** Backend/validation failure that teaches the fix. Raw JSON alone is a bug. */
export function renderErrorCard(
	failure: PlaygroundFailure,
	hooks: ErrorHooks,
): HTMLElement {
	const card = h(
		"div",
		{
			className:
				"grid gap-2 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] border-l-2 border-l-[var(--q-bad)] bg-[var(--sl-color-bg)] p-3",
			dataset: { errorCard: failure.code ?? "unknown" },
		},
		h(
			"p",
			{
				className:
					"m-0 font-mono text-[0.6875rem] font-semibold uppercase tracking-[0.12em] text-[var(--q-bad)]",
			},
			failure.code === "QQL-PARSE-SYNTAX" ||
				failure.kind === "Parse" ||
				failure.kind === "Lex"
				? "Invalid QQL"
				: "Run failed",
		),
		h(
			"p",
			{
				className:
					"m-0 text-[0.8125rem] leading-6 font-medium text-[var(--sl-color-gray-1)]",
			},
			failure.message,
		),
	);
	const fix = fixSuggestion(failure, hooks);
	if (fix) card.append(fix);

	if (failure.code === "QQL-MISSING-USING") {
		const slot = h(
			"div",
			{ className: "grid gap-2", dataset: { usingSlot: "" } },
			h(
				"p",
				{
					className:
						"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
				},
				"Looking up this collection's vectors…",
			),
		);
		card.append(slot);
		void fillUsingSlot(slot, failure, hooks);
	}

	const rows: Array<[string, string]> = [];
	if (failure.code) rows.push(["code", failure.code]);
	if (failure.kind) rows.push(["kind", failure.kind]);
	if (failure.span)
		rows.push(["span", `${failure.span.start}..${failure.span.end}`]);
	for (const [key, value] of Object.entries(failure.fields))
		rows.push([`field:${key}`, value]);
	const details = h(
		"details",
		{ className: "pg-details grid gap-2" },
		h(
			"summary",
			{
				className:
					"cursor-pointer font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]",
			},
			failure.code
				? `${failure.code} — technical details`
				: "Technical details",
		),
	);
	if (rows.length > 0) {
		const meta = h("dl", {
			className: "m-0 grid gap-1 font-mono text-[0.6875rem]",
		});
		for (const [term, definition] of rows) {
			meta.append(
				h(
					"div",
					{ className: "flex gap-2" },
					h(
						"dt",
						{ className: "m-0 shrink-0 text-[var(--sl-color-gray-3)]" },
						term,
					),
					h(
						"dd",
						{
							className: "m-0 min-w-0 break-all text-[var(--sl-color-gray-1)]",
						},
						definition,
					),
				),
			);
		}
		details.append(meta);
	}
	details.append(
		h("pre", {
			className:
				"pg-hl m-0 overflow-auto rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem] leading-5 whitespace-pre-wrap break-words text-[var(--sl-color-gray-2)]",
			html: highlightJson(prettyRaw(failure.raw)),
		}),
	);
	card.append(details);
	return card;
}

/** One-click USING fix: buttons are user-confirmed, never auto-applied. */
async function fillUsingSlot(
	slot: HTMLElement,
	failure: PlaygroundFailure,
	hooks: ErrorHooks,
): Promise<void> {
	const collection = /Collection '([^']+)'/.exec(failure.raw)?.[1] ?? null;
	if (!collection) {
		slot.replaceChildren();
		return;
	}
	let topology: LiveCollectionTopology | null = null;
	try {
		topology = await hooks.resolveTopology(collection);
	} catch {
		topology = null;
	}
	slot.replaceChildren();
	if (!topology || topology.dense.length === 0) {
		const names = /Available vectors: ([^\n"]+)/.exec(failure.raw)?.[1];
		slot.append(
			h(
				"p",
				{
					className:
						"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
				},
				names
					? `No dense vector found. Available vectors: ${names}. Add USING <name> by hand.`
					: "Couldn't read this collection's vectors. Add USING <name> by hand.",
			),
		);
		return;
	}
	const targets =
		topology.dense.length === 1
			? topology.dense
			: topology.dense.filter((name) => name === "dense");
	const choices = targets.length > 0 ? targets : topology.dense;
	if (topology.dense.length > 1 && targets.length === 0) {
		slot.append(
			h(
				"p",
				{
					className:
						"m-0 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]",
				},
				"Several dense vectors exist — pick the one this text should search.",
			),
		);
	}
	for (const vector of choices) {
		const apply = h(
			"button",
			{
				type: "button",
				className: "app-action app-action--lg w-fit",
				dataset: { usingApply: vector },
			},
			`Apply USING ${vector} AS DENSE`,
		);
		apply.addEventListener("click", () =>
			hooks.onApplyUsing(collection, vector),
		);
		slot.append(apply);
	}
}
