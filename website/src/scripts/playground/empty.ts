import { h } from "./dom";

export type EmptyKind = "result" | "offline" | "wire" | "examples";

/** Shimmering HitCard shells: first paint before the first run. */
export function resultSkeleton(): HTMLElement {
	const root = h("div", {
		className: "grid content-start gap-3",
		ariaHidden: "true",
	});
	for (let index = 0; index < 3; index += 1) {
		root.append(
			h(
				"div",
				{
					className:
						"grid content-start gap-2 rounded-[var(--q-r)] border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3",
				},
				h("div", {
					className:
						"h-4 w-2/3 animate-pulse rounded bg-[var(--sl-color-gray-5)]",
				}),
				h("div", {
					className:
						"h-6 w-1/2 animate-pulse rounded bg-[var(--sl-color-gray-5)]",
				}),
				h("div", {
					className:
						"h-3 w-full animate-pulse rounded bg-[var(--sl-color-gray-5)]",
				}),
				h("div", {
					className:
						"h-3 w-5/6 animate-pulse rounded bg-[var(--sl-color-gray-5)]",
				}),
			),
		);
	}
	return root;
}

/** Every tab earns its empty: kicker + one line + one action. */
export function emptyState(
	kind: EmptyKind,
	action: { label: string; run: () => void } | null,
	customBody?: string,
): HTMLElement {
	const bodies: Record<EmptyKind, string> = {
		result: "Run the demo query — ⌘↵",
		offline:
			"You're offline-capable: Plan, Wire, AST, Explain all work without a backend. Connect Qdrant to run.",
		wire: "Wire appears after a valid parse.",
		examples: customBody ?? "No fixture matches — clear search.",
	};
	const kickers: Record<EmptyKind, string> = {
		result: "Result",
		offline: "Offline",
		wire: "Wire",
		examples: "Examples",
	};
	const box = h(
		"div",
		{
			className:
				"grid content-center justify-items-center gap-2 p-8 text-center",
			role: "status",
		},
		h("p", { className: "mono-label m-0" }, kickers[kind]),
		h(
			"p",
			{
				className:
					"m-0 max-w-64 font-mono text-[0.72rem] leading-relaxed text-[var(--sl-color-gray-3)]",
			},
			bodies[kind],
		),
	);
	if (action) {
		const btn = h(
			"button",
			{ type: "button", className: "app-action app-action--lg" },
			action.label,
		);
		btn.addEventListener("click", action.run);
		box.append(btn);
	}
	return box;
}
