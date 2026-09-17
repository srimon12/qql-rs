import type { PlaygroundFailure } from "./types";

export function escapeHtml(str: unknown): string {
	if (str == null) return "";
	return String(str)
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#039;");
}

export function pretty(value: unknown): string {
	if (value === undefined) return "";
	if (typeof value === "string") return value;
	try {
		return JSON.stringify(
			value,
			(_key, val) => (typeof val === "bigint" ? Number(val) : val),
			2,
		);
	} catch {
		return String(value);
	}
}

export function highlightJson(json: string): string {
	const escaped = escapeHtml(json);
	return escaped.replace(
		/("(?:\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?)/g,
		(match) => {
			let cls = "tok-num";
			if (/^"/.test(match)) {
				cls = /:$/.test(match) ? "tok-key" : "tok-str";
			} else if (/true|false|null/.test(match)) {
				cls = "tok-lit";
			}
			return `<span class="${cls}">${match}</span>`;
		},
	);
}

export interface ScoredPoint {
	id: string | number | bigint;
	score?: number;
	payload?: Record<string, unknown> | null;
	vector?: unknown;
}

export function renderEmpty(container: HTMLElement, message?: string): void {
	container.innerHTML = `
		<div class="flex h-full min-h-[16rem] flex-col items-center justify-center p-8 text-center">
			<div class="flex size-10 items-center justify-center rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] text-[var(--sl-color-gray-3)]">
				<svg class="size-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
					<polygon points="5 3 19 12 5 21 5 3"></polygon>
				</svg>
			</div>
			<p class="mt-3 font-mono text-xs font-semibold uppercase tracking-wider text-[var(--sl-color-gray-2)]">Live Execution</p>
			<p class="mt-1 max-w-sm font-mono text-xs leading-relaxed text-[var(--sl-color-gray-3)]">
				${escapeHtml(message || "The Result tab connects to your live Qdrant instance. Run a query to view live hits, payload properties, and similarity scores.")}
			</p>
		</div>
	`;
}

export function renderError(
	container: HTMLElement,
	failure: PlaygroundFailure,
): void {
	const codeBadge = failure.code
		? `<span class="inline-flex items-center rounded border border-[color-mix(in_srgb,var(--q-bad)_40%,transparent)] bg-[color-mix(in_srgb,var(--q-bad)_15%,transparent)] px-2 py-0.5 font-mono text-[0.6875rem] font-bold text-[var(--q-bad)]">${escapeHtml(failure.code)}</span>`
		: "";

	container.innerHTML = `
		<div class="rounded-lg border border-[color-mix(in_srgb,var(--q-bad)_40%,var(--sl-color-gray-5))] bg-[color-mix(in_srgb,var(--q-bad)_5%,var(--sl-color-bg))] p-4 text-[var(--sl-color-gray-1)]">
			<div class="flex items-start gap-3">
				<div class="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-[color-mix(in_srgb,var(--q-bad)_20%,transparent)] text-[var(--q-bad)]">
					<svg class="size-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<circle cx="12" cy="12" r="10"></circle>
						<line x1="12" y1="8" x2="12" y2="12"></line>
						<line x1="12" y1="16" x2="12.01" y2="16"></line>
					</svg>
				</div>
				<div class="min-w-0 flex-1">
					<div class="flex flex-wrap items-center gap-2">
						<span class="font-semibold text-xs tracking-wide text-[var(--sl-color-white)]">Query Execution Failed</span>
						${codeBadge}
					</div>
					<p class="mt-2 text-xs font-mono leading-relaxed text-[var(--sl-color-gray-2)]">${escapeHtml(failure.message)}</p>
					
					${
						failure.raw && failure.raw !== failure.message
							? `
						<details class="mt-3 rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-2.5 font-mono text-[0.6875rem]">
							<summary class="cursor-pointer text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]">Raw error details</summary>
							<pre class="mt-2 overflow-x-auto whitespace-pre-wrap break-words text-[var(--sl-color-gray-3)]">${escapeHtml(failure.raw)}</pre>
						</details>
					`
							: ""
					}
				</div>
			</div>
		</div>
	`;
}

export function renderHits(
	container: HTMLElement,
	hits: ScoredPoint[],
	timingMs: number | null,
): void {
	if (!hits || hits.length === 0) {
		renderEmpty(container, "Query returned 0 points.");
		return;
	}

	const maxScore = Math.max(...hits.map((h) => h.score ?? 0), 1.0);
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";

	const hitsHtml = hits
		.map((hit, index) => {
			const score = hit.score ?? 0;
			const scorePct = Math.max(
				0,
				Math.min(100, Math.round((score / maxScore) * 100)),
			);
			const payload = hit.payload;
			const hasPayload =
				payload &&
				typeof payload === "object" &&
				Object.keys(payload).length > 0;

			let title = "";
			const items: Array<[string, unknown]> = [];
			if (hasPayload) {
				for (const [k, v] of Object.entries(payload)) {
					if (!title && (k === "name" || k === "title" || k === "text")) {
						title = String(v);
					} else {
						items.push([k, v]);
					}
				}
			}

			return `
				<div class="hit-enter rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-3.5 transition-colors hover:border-[var(--sl-color-gray-4)]">
					<div class="flex items-center justify-between gap-3">
						<div class="flex items-center gap-2">
							<span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-[var(--sl-color-gray-6)] font-mono text-[0.65rem] font-bold text-[var(--sl-color-gray-3)]">#${index + 1}</span>
							<span class="font-mono text-xs font-semibold text-[var(--sl-color-white)]">id: ${escapeHtml(String(hit.id))}</span>
						</div>
						<div class="flex items-center gap-2">
							<div class="flex items-center gap-1.5 rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-2 py-0.5 font-mono text-[0.6875rem]">
								<span class="text-[var(--sl-color-gray-3)]">score</span>
								<span class="font-bold text-[var(--sl-color-accent-high)]">${score.toFixed(4)}</span>
							</div>
							<button class="app-icon-btn copy-btn--reveal" type="button" data-copy-json="${escapeHtml(pretty(hit))}" title="Copy point JSON" aria-label="Copy point JSON">
								<svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
							</button>
						</div>
					</div>

					<div class="mt-2 h-1 w-full overflow-hidden rounded-full bg-[var(--sl-color-gray-6)]">
						<div class="scorebar-fill h-full bg-[var(--sl-color-accent)]" style="width: ${scorePct}%;"></div>
					</div>

					${
						title
							? `<p class="mt-2.5 text-xs font-medium text-[var(--sl-color-gray-1)] leading-relaxed">${escapeHtml(title)}</p>`
							: ""
					}

					${
						items.length > 0
							? `
						<dl class="mt-2.5 grid grid-cols-1 gap-1 border-t border-[var(--sl-color-gray-5)] pt-2 text-[0.72rem] sm:grid-cols-2">
							${items
								.slice(0, 8)
								.map(
									([k, v]) => `
								<div class="flex items-baseline justify-between gap-2 overflow-hidden px-1 py-0.5 font-mono">
									<dt class="shrink-0 text-[var(--sl-color-gray-3)]">${escapeHtml(k)}:</dt>
									<dd class="truncate text-right text-[var(--sl-color-gray-2)]" title="${escapeHtml(String(v))}">${escapeHtml(typeof v === "object" ? JSON.stringify(v) : String(v))}</dd>
								</div>
							`,
								)
								.join("")}
						</dl>
					`
							: ""
					}
				</div>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-2 font-mono text-[0.72rem]">
				<span class="font-bold text-[var(--sl-color-white)]">${hits.length} ${hits.length === 1 ? "Hit" : "Hits"}</span>
				<div class="flex items-center gap-3">
					${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">${escapeHtml(timingStr)}</span>` : ""}
					<button class="app-action px-2" type="button" data-copy-json="${escapeHtml(pretty(hits))}">
						Copy all JSON
					</button>
				</div>
			</div>
			<div class="grid gap-2.5">
				${hitsHtml}
			</div>
		</div>
	`;
}

export function renderCount(
	container: HTMLElement,
	count: number,
	timingMs: number | null,
): void {
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";
	container.innerHTML = `
		<div class="hit-enter flex flex-col items-center justify-center rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-8 text-center">
			<p class="font-mono text-xs font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Point Count</p>
			<p class="mt-2 font-mono text-3xl font-extrabold text-[var(--sl-color-accent-high)]">${count.toLocaleString()}</p>
			<p class="mt-2 font-mono text-xs text-[var(--sl-color-gray-3)]">Matched points in collection ${timingStr ? `· ${timingStr}` : ""}</p>
		</div>
	`;
}

export function renderFacets(
	container: HTMLElement,
	facets: Array<{ value: unknown; count: number }>,
	timingMs: number | null,
): void {
	if (!facets || facets.length === 0) {
		renderEmpty(container, "No facet values returned.");
		return;
	}
	const maxCount = Math.max(...facets.map((f) => f.count), 1);
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";

	const rows = facets
		.map((facet) => {
			const pct = Math.round((facet.count / maxCount) * 100);
			return `
				<div class="flex items-center justify-between gap-3 rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] px-3 py-2 font-mono text-xs">
					<span class="truncate font-semibold text-[var(--sl-color-gray-1)]">${escapeHtml(String(facet.value))}</span>
					<div class="flex items-center gap-3">
						<div class="h-1.5 w-24 overflow-hidden rounded-full bg-[var(--sl-color-gray-6)]">
							<div class="h-full bg-[var(--sl-color-accent)]" style="width: ${pct}%;"></div>
						</div>
						<span class="w-12 text-right font-bold text-[var(--sl-color-accent-high)]">${facet.count.toLocaleString()}</span>
					</div>
				</div>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-2 font-mono text-xs">
				<span class="font-bold text-[var(--sl-color-white)]">${facets.length} Facets</span>
				${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">${escapeHtml(timingStr)}</span>` : ""}
			</div>
			<div class="grid gap-1.5">
				${rows}
			</div>
		</div>
	`;
}

export function renderMutation(
	container: HTMLElement,
	operation: string,
	message: string,
	timingMs: number | null,
): void {
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";
	container.innerHTML = `
		<div class="hit-enter rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] p-6">
			<div class="flex items-center gap-2">
				<span class="rounded bg-[color-mix(in_srgb,var(--q-ok)_15%,transparent)] px-2 py-0.5 font-mono text-xs font-bold text-[var(--q-ok)]">Success</span>
				<span class="font-mono text-xs font-semibold text-[var(--sl-color-white)]">${escapeHtml(operation)}</span>
			</div>
			<p class="mt-3 font-mono text-xs leading-relaxed text-[var(--sl-color-gray-2)]">${escapeHtml(message)}</p>
			${timingStr ? `<p class="mt-2 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]">Duration: ${escapeHtml(timingStr)}</p>` : ""}
		</div>
	`;
}
