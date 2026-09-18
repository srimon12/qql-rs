import { escapeHtml } from "../core/dom";
import type { PlaygroundFailure, PlaygroundMetrics } from "../core/types";

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
	shard_key?: string | number | null;
}

export function renderEmpty(container: HTMLElement, message?: string): void {
	container.innerHTML = `
		<div class="flex h-full min-h-[16rem] flex-col items-center justify-center p-8 text-center">
			<div class="flex size-11 items-center justify-center rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] text-[var(--sl-color-gray-3)] shadow-xs">
				<svg class="size-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
					<polygon points="5 3 19 12 5 21 5 3"></polygon>
				</svg>
			</div>
			<p class="mt-3.5 font-mono text-xs font-semibold uppercase tracking-wider text-[var(--sl-color-gray-2)]">Live Result Console</p>
			<p class="mt-1.5 max-w-sm font-mono text-xs leading-relaxed text-[var(--sl-color-gray-3)]">
				${escapeHtml(message || "Run a query against your reachable Qdrant instance (⌘↵ / Ctrl+Enter) to view live similarity hits, payload fields, and execution telemetry.")}
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
		<div class="rounded-lg border border-[color-mix(in_srgb,var(--q-bad)_40%,var(--sl-color-gray-5))] bg-[color-mix(in_srgb,var(--q-bad)_5%,var(--sl-color-bg))] p-4 text-[var(--sl-color-gray-1)] shadow-xs">
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

function renderPayloadValue(v: unknown): string {
	if (v === null)
		return `<span class="text-[var(--sl-color-gray-4)] italic">null</span>`;
	if (typeof v === "boolean")
		return `<span class="font-semibold ${v ? "text-[var(--q-ok)]" : "text-[var(--q-bad)]"}">${v}</span>`;
	if (typeof v === "number")
		return `<span class="font-semibold text-[var(--q-sx-number)]">${v}</span>`;
	if (typeof v === "string")
		return `<span class="text-[var(--sl-color-gray-1)]">"${escapeHtml(v)}"</span>`;
	if (Array.isArray(v)) {
		if (v.length === 0)
			return `<span class="text-[var(--sl-color-gray-4)]">[]</span>`;
		if (typeof v[0] === "string" || typeof v[0] === "number") {
			return `<span class="text-[var(--sl-color-gray-2)]">[${v
				.slice(0, 4)
				.map((x) => String(x))
				.join(", ")}${v.length > 4 ? `… +${v.length - 4}` : ""}]</span>`;
		}
		return `<span class="text-[var(--sl-color-gray-3)]">Array(${v.length})</span>`;
	}
	if (typeof v === "object") {
		return `<span class="text-[var(--sl-color-gray-3)]">${escapeHtml(JSON.stringify(v))}</span>`;
	}
	return escapeHtml(String(v));
}

export function renderHits(
	container: HTMLElement,
	hits: ScoredPoint[],
	timingMs: number | null,
	collectionName?: string,
): void {
	if (!hits || hits.length === 0) {
		renderEmpty(container, "Query returned 0 points.");
		return;
	}

	const maxScore = Math.max(...hits.map((h) => h.score ?? 0), 0.0001);
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

			// Vector snippet
			let vectorInfo = "";
			if (hit.vector) {
				if (Array.isArray(hit.vector)) {
					vectorInfo = `Dense vector · ${hit.vector.length} dims`;
				} else if (typeof hit.vector === "object") {
					vectorInfo = `Named / Sparse vector`;
				}
			}

			const hitJson = escapeHtml(pretty(hit));

			return `
				<article class="hit-enter group relative rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)] hover:shadow-xs">
					<!-- Top Row: Rank, ID, Score, Actions -->
					<div class="flex flex-wrap items-center justify-between gap-2.5">
						<div class="flex min-w-0 items-center gap-2">
							<span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-[var(--sl-color-gray-7)] font-mono text-[0.625rem] font-bold text-[var(--sl-color-gray-3)]">
								#${index + 1}
							</span>
							<div class="flex min-w-0 items-center gap-1.5 font-mono text-xs">
								<span class="text-[var(--sl-color-gray-4)]">id:</span>
								<strong class="max-w-32 truncate font-semibold text-[var(--sl-color-white)] sm:max-w-56" title="${escapeHtml(String(hit.id))}">${escapeHtml(String(hit.id))}</strong>
								<button class="app-icon-btn opacity-60 transition-opacity hover:opacity-100" type="button" data-copy-text="${escapeHtml(String(hit.id))}" title="Copy ID" aria-label="Copy ID">
									<svg class="size-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
								</button>
							</div>
							${
								hit.shard_key != null
									? `<span class="rounded bg-[var(--sl-color-gray-7)] px-1.5 py-0.5 font-mono text-[0.625rem] text-[var(--sl-color-gray-3)]">shard: ${escapeHtml(String(hit.shard_key))}</span>`
									: ""
							}
						</div>

						<div class="flex items-center gap-2">
							<div class="flex items-center gap-1.5 rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-bg)] px-2.5 py-0.5 font-mono text-[0.6875rem] whitespace-nowrap">
								<span class="text-[var(--sl-color-gray-3)]">score</span>
								<strong class="font-bold text-[var(--sl-color-accent-high)]">${score.toFixed(4)}</strong>
								<span class="text-[0.625rem] text-[var(--sl-color-gray-4)]">(${scorePct}%)</span>
							</div>
							<button class="app-action h-6 px-2 text-[0.6875rem] font-medium whitespace-nowrap" type="button" data-copy-json="${hitJson}" title="Copy single point JSON">
								Copy JSON
							</button>
						</div>
					</div>

					<!-- Visual Score Bar -->
					<div class="mt-2.5 h-1 w-full overflow-hidden rounded-full bg-[var(--sl-color-gray-5)]/60">
						<div class="h-full rounded-full bg-[var(--sl-color-accent)] transition-all" style="width: ${scorePct}%;"></div>
					</div>

					<!-- Title Highlight if found -->
					${
						title
							? `<h3 class="mt-3 text-sm font-semibold text-[var(--sl-color-white)] leading-snug line-clamp-2">${escapeHtml(title)}</h3>`
							: ""
					}

					<!-- Payload Key-Value Grid -->
					${
						items.length > 0
							? `
						<div class="mt-3 rounded-lg border border-[var(--sl-color-gray-5)]/80 bg-[var(--sl-color-bg)]/80 p-2.5">
							<div class="grid grid-cols-1 gap-1 font-mono text-[0.7rem]">
								${items
									.slice(0, 10)
									.map(
										([k, v]) => `
									<div class="flex items-baseline justify-between gap-2 overflow-hidden px-1.5 py-1 border-b border-[var(--sl-color-gray-5)]/30 last:border-b-0">
										<dt class="shrink-0 font-medium text-[var(--sl-color-gray-3)]">${escapeHtml(k)}:</dt>
										<dd class="truncate text-right font-mono text-[var(--sl-color-gray-1)]" title="${escapeHtml(String(v))}">${renderPayloadValue(v)}</dd>
									</div>
								`,
									)
									.join("")}
							</div>
							${
								items.length > 10
									? `<p class="mt-1 text-right font-mono text-[0.625rem] text-[var(--sl-color-gray-4)]">+${items.length - 10} more fields</p>`
									: ""
							}
						</div>
					`
							: ""
					}

					<!-- Vector Preview (Expandable) -->
					${
						hit.vector
							? `
						<details class="mt-2 text-xs">
							<summary class="cursor-pointer font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)] hover:text-[var(--sl-color-gray-1)]">
								${escapeHtml(vectorInfo || "Vector data")}
							</summary>
							<pre class="pg-hl mt-1.5 max-h-28 overflow-auto rounded bg-[var(--sl-color-bg)] p-2 font-mono text-[0.65rem] text-[var(--sl-color-gray-2)]">${escapeHtml(pretty(hit.vector))}</pre>
						</details>
					`
							: ""
					}
				</article>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<!-- Results Header Strip -->
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-3 font-mono text-[0.75rem]">
				<div class="flex items-center gap-2">
					<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-2.5 py-0.5 font-bold text-[var(--sl-color-accent-high)]">
						${hits.length} ${hits.length === 1 ? "Result" : "Results"}
					</span>
					${collectionName ? `<span class="text-[var(--sl-color-gray-3)]">from <strong class="text-[var(--sl-color-gray-1)]">${escapeHtml(collectionName)}</strong></span>` : ""}
				</div>
				<div class="flex items-center gap-3">
					${timingStr ? `<span class="flex items-center gap-1 text-[var(--sl-color-gray-3)]"><span class="text-[var(--q-ok)]">⚡</span> ${escapeHtml(timingStr)}</span>` : ""}
					<button class="app-action h-7 px-2.5 text-xs font-medium" type="button" data-copy-json="${escapeHtml(pretty(hits))}">
						Copy all JSON
					</button>
				</div>
			</div>

			<!-- Hit Cards List -->
			<div class="grid gap-3">
				${hitsHtml}
			</div>
		</div>
	`;
}

export function renderCount(
	container: HTMLElement,
	count: number,
	timingMs: number | null,
	collectionName?: string,
): void {
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";
	container.innerHTML = `
		<div class="hit-enter flex flex-col items-center justify-center rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-10 text-center shadow-xs">
			<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-3 py-1 font-mono text-xs font-bold uppercase tracking-wider text-[var(--sl-color-accent-high)]">
				Point Count
			</span>
			<p class="mt-4 font-mono text-4xl font-extrabold tracking-tight text-[var(--sl-color-white)] sm:text-5xl">
				${count.toLocaleString()}
			</p>
			<p class="mt-3 font-mono text-xs text-[var(--sl-color-gray-3)]">
				Matched points in collection ${collectionName ? `<strong>${escapeHtml(collectionName)}</strong>` : ""} ${timingStr ? `· ⚡ ${timingStr}` : ""}
			</p>
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
				<div class="flex items-center justify-between gap-3 rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-3.5 py-2.5 font-mono text-xs transition-colors hover:border-[var(--sl-color-gray-4)]">
					<span class="truncate font-semibold text-[var(--sl-color-gray-1)]">${escapeHtml(String(facet.value))}</span>
					<div class="flex items-center gap-3">
						<div class="h-2 w-32 overflow-hidden rounded-full bg-[var(--sl-color-gray-5)]">
							<div class="h-full rounded-full bg-[var(--sl-color-accent)]" style="width: ${pct}%;"></div>
						</div>
						<span class="w-14 text-right font-bold text-[var(--sl-color-accent-high)]">${facet.count.toLocaleString()}</span>
					</div>
				</div>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-3 font-mono text-xs">
				<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-2.5 py-0.5 font-bold text-[var(--sl-color-accent-high)]">
					${facets.length} Facets
				</span>
				${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">⚡ ${escapeHtml(timingStr)}</span>` : ""}
			</div>
			<div class="grid gap-2">
				${rows}
			</div>
		</div>
	`;
}

export function renderGroups(
	container: HTMLElement,
	groups: Array<{ id: unknown; hits: ScoredPoint[] }>,
	timingMs: number | null,
): void {
	if (!groups || groups.length === 0) {
		renderEmpty(container, "No grouped results returned.");
		return;
	}
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";

	const groupsHtml = groups
		.map((g, idx) => {
			return `
				<div class="rounded-lg border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-3.5">
					<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-2 font-mono text-xs">
						<div class="flex items-center gap-2">
							<span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-[var(--sl-color-gray-7)] font-bold text-[var(--sl-color-gray-3)]">G${idx + 1}</span>
							<strong class="text-[var(--sl-color-white)]">Group: ${escapeHtml(String(g.id))}</strong>
						</div>
						<span class="text-[var(--sl-color-gray-3)]">${g.hits?.length ?? 0} hits</span>
					</div>
					<div class="mt-2.5 grid gap-2" data-group-hits-${idx}></div>
				</div>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-3 font-mono text-xs">
				<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-2.5 py-0.5 font-bold text-[var(--sl-color-accent-high)]">
					${groups.length} Groups
				</span>
				${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">⚡ ${escapeHtml(timingStr)}</span>` : ""}
			</div>
			<div class="grid gap-3">
				${groupsHtml}
			</div>
		</div>
	`;

	// Fill hits inside each group
	groups.forEach((g, idx) => {
		const target = container.querySelector<HTMLElement>(
			`[data-group-hits-${idx}]`,
		);
		if (target && g.hits && g.hits.length > 0) {
			renderHits(target, g.hits, null);
		}
	});
}

export function renderCollections(
	container: HTMLElement,
	collections: string[],
	timingMs: number | null,
): void {
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";
	const cards = collections
		.map(
			(col) => `
		<div class="flex items-center justify-between gap-2.5 rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-3.5 transition-colors hover:border-[var(--sl-color-gray-4)]">
			<div class="flex min-w-0 items-center gap-2.5">
				<div class="flex size-7 shrink-0 items-center justify-center rounded-md bg-[var(--sl-color-gray-7)] font-mono text-xs font-bold text-[var(--sl-color-accent-high)]">
					⛁
				</div>
				<strong class="truncate font-mono text-xs font-semibold text-[var(--sl-color-white)]">${escapeHtml(col)}</strong>
			</div>
			<button class="app-action h-7 px-2.5 text-xs whitespace-nowrap shrink-0" type="button" data-load-query="SCROLL FROM ${escapeHtml(col)} LIMIT 5;">
				Scroll points →
			</button>
		</div>
	`,
		)
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-3 font-mono text-xs">
				<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-2.5 py-0.5 font-bold text-[var(--sl-color-accent-high)]">
					${collections.length} Collections
				</span>
				${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">⚡ ${escapeHtml(timingStr)}</span>` : ""}
			</div>
			<div class="grid gap-2.5 grid-cols-1">
				${cards}
			</div>
		</div>
	`;
}

export function renderShardKeys(
	container: HTMLElement,
	shardKeys: unknown[],
	timingMs: number | null,
): void {
	const timingStr = timingMs != null ? `${timingMs.toFixed(1)}ms` : "";
	const pills = shardKeys
		.map(
			(key) => `
		<span class="inline-flex items-center gap-1.5 rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-3 py-1 font-mono text-xs font-medium text-[var(--sl-color-gray-1)]">
			<span class="size-1.5 rounded-full bg-[var(--sl-color-accent)]"></span>
			${escapeHtml(typeof key === "object" && key != null && "key" in key ? String((key as { key: unknown }).key) : String(key))}
		</span>
	`,
		)
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-3">
			<div class="flex items-center justify-between border-b border-[var(--sl-color-gray-5)] pb-3 font-mono text-xs">
				<span class="rounded-full bg-[var(--sl-color-accent)]/15 px-2.5 py-0.5 font-bold text-[var(--sl-color-accent-high)]">
					${shardKeys.length} Shard Keys
				</span>
				${timingStr ? `<span class="text-[var(--sl-color-gray-3)]">⚡ ${escapeHtml(timingStr)}</span>` : ""}
			</div>
			<div class="flex flex-wrap gap-2">
				${pills}
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
		<div class="hit-enter rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-6 shadow-xs">
			<div class="flex items-center gap-2.5">
				<span class="flex size-6 items-center justify-center rounded-full bg-[color-mix(in_srgb,var(--q-ok)_18%,transparent)] text-xs font-bold text-[var(--q-ok)]">
					✓
				</span>
				<span class="font-mono text-xs font-bold uppercase tracking-wider text-[var(--sl-color-white)]">${escapeHtml(operation)}</span>
				<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.6875rem] text-[var(--q-ok)]">SUCCESS</span>
			</div>
			<p class="mt-3 font-mono text-xs leading-relaxed text-[var(--sl-color-gray-2)]">${escapeHtml(message)}</p>
			${timingStr ? `<p class="mt-2 font-mono text-[0.6875rem] text-[var(--sl-color-gray-3)]">⚡ Execution: ${escapeHtml(timingStr)}</p>` : ""}
		</div>
	`;
}

export interface ExecResponseLike {
	ok?: boolean | null;
	operation?: string | null;
	message?: string | null;
	data?: unknown;
}

/**
 * Route one executor response into the Result panel and report its badge
 * count (hits, groups, collections, …) — `null` when the shape is not
 * countable (mutations, empty payloads).
 */
export function renderExecResponse(
	container: HTMLElement,
	response: ExecResponseLike | null,
	executeMs: number | null,
): number | null {
	const data = response?.data ?? null;

	if (Array.isArray(data)) {
		if (
			data.length > 0 &&
			typeof data[0] === "object" &&
			data[0] !== null &&
			"value" in data[0] &&
			"count" in data[0]
		) {
			const facets = data as Array<{ value: unknown; count: number }>;
			renderFacets(container, facets, executeMs);
			return facets.length;
		}
		if (data.length > 0 && typeof data[0] === "string") {
			renderCollections(container, data as string[], executeMs);
			return data.length;
		}
		const hits = data as ScoredPoint[];
		renderHits(container, hits, executeMs);
		return hits.length;
	}

	if (data && typeof data === "object") {
		if (
			typeof (data as { count?: unknown }).count === "number" ||
			typeof (data as { count?: unknown }).count === "bigint"
		) {
			const count = Number((data as { count: number | bigint }).count);
			renderCount(container, count, executeMs);
			return count;
		}
		if (Array.isArray((data as { groups?: unknown }).groups)) {
			const groups = (
				data as { groups: Array<{ id: unknown; hits: ScoredPoint[] }> }
			).groups;
			renderGroups(container, groups, executeMs);
			return groups.length;
		}
		if (Array.isArray((data as { collections?: unknown }).collections)) {
			const collections = (data as { collections: string[] }).collections;
			renderCollections(container, collections, executeMs);
			return collections.length;
		}
		if (Array.isArray((data as { shard_keys?: unknown }).shard_keys)) {
			const shardKeys = (data as { shard_keys: unknown[] }).shard_keys;
			renderShardKeys(container, shardKeys, executeMs);
			return shardKeys.length;
		}
		renderMutation(
			container,
			response?.operation ?? "SUCCESS",
			response?.message ?? "Operation completed.",
			executeMs,
		);
		return null;
	}

	if (response) {
		renderMutation(
			container,
			response.operation ?? "SUCCESS",
			response.message ?? "Operation completed successfully.",
			executeMs,
		);
		return null;
	}

	renderEmpty(container, "Query executed successfully.");
	return null;
}

export function renderMetrics(
	container: HTMLElement,
	metrics: PlaygroundMetrics | null,
	telemetry: Record<string, unknown> | null | undefined,
): void {
	if (!container) return;

	const latencyNum =
		metrics?.executeMs != null ? metrics.executeMs.toFixed(1) : "—";
	const parseNum = metrics?.parseMs != null ? metrics.parseMs.toFixed(2) : "—";
	const stmtsVal =
		metrics?.statements != null ? String(metrics.statements) : "1";

	const rawServerTime =
		typeof telemetry?.time_s === "number"
			? telemetry.time_s * 1000
			: typeof telemetry?.time === "number"
				? telemetry.time * 1000
				: null;
	const serverNum = rawServerTime != null ? rawServerTime.toFixed(1) : "—";

	let embedTitle = "None (Raw)";
	let embedSub = "No runtime embedding inference";
	let embedBadge = "DIRECT";
	if (metrics?.embedProvider === "browser") {
		embedTitle = "MiniLM (384d)";
		embedSub = "In-browser ONNX Web runtime";
		embedBadge = "BROWSER";
	} else if (metrics?.embedProvider === "http") {
		embedTitle = "HTTP Model";
		embedSub = "OpenAI-compatible embeddings";
		embedBadge = "HTTP";
	}

	const isPolicyActive = Boolean(metrics?.policyApplied);
	const policyTitle = isPolicyActive ? "Policy Injected" : "Disabled";
	const policySub = isPolicyActive
		? "Row-level tenant isolation applied"
		: "No tenant isolation configured";
	const policyBadge = isPolicyActive ? "ACTIVE" : "INACTIVE";
	const policyDotClass = isPolicyActive
		? "bg-[var(--q-ok)] shadow-xs animate-pulse"
		: "bg-[var(--sl-color-gray-4)]";
	const policyBadgeClass = isPolicyActive
		? "bg-[color-mix(in_srgb,var(--q-ok)_18%,transparent)] text-[var(--q-ok)] border border-[color-mix(in_srgb,var(--q-ok)_30%,transparent)]"
		: "bg-[var(--sl-color-gray-7)] text-[var(--sl-color-gray-3)]";

	container.innerHTML = `
		<!-- Metrics Stat Cards (2 Columns for Proper Width & Readability) -->
		<div class="hit-enter grid grid-cols-1 gap-3 sm:grid-cols-2">
			<!-- Card 1: Execution Latency -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Roundtrip Latency</span>
					<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-accent-high)]">ROUNDTRIP</span>
				</div>
				<div class="mt-2.5 flex items-baseline gap-1.5 font-mono whitespace-nowrap">
					<span class="text-2xl font-bold tracking-tight text-[var(--sl-color-white)]">${latencyNum}</span>
					${latencyNum !== "—" ? `<span class="text-xs font-semibold text-[var(--sl-color-gray-3)]">ms</span>` : ""}
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">Full client request to decode</p>
			</div>

			<!-- Card 2: Server Engine Time -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Server Engine Time</span>
					<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-gray-3)]">QDRANT</span>
				</div>
				<div class="mt-2.5 flex items-baseline gap-1.5 font-mono whitespace-nowrap">
					<span class="text-2xl font-bold tracking-tight ${serverNum !== "—" ? "text-[var(--sl-color-white)]" : "text-[var(--sl-color-gray-4)]"}">${serverNum}</span>
					${serverNum !== "—" ? `<span class="text-xs font-semibold text-[var(--sl-color-gray-3)]">ms</span>` : ""}
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">Reported search execution time</p>
			</div>

			<!-- Card 3: WASM Compiler -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">WASM Compiler</span>
					<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-accent-high)]">OFFLINE</span>
				</div>
				<div class="mt-2.5 flex items-baseline gap-1.5 font-mono whitespace-nowrap">
					<span class="text-2xl font-bold tracking-tight text-[var(--sl-color-white)]">${parseNum}</span>
					<span class="text-xs font-semibold text-[var(--sl-color-gray-3)]">ms</span>
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">In-browser parser &amp; planner</p>
			</div>

			<!-- Card 4: Statements -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Statements</span>
					<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-gray-3)]">SCRIPT</span>
				</div>
				<div class="mt-2.5 flex items-baseline gap-1.5 font-mono whitespace-nowrap">
					<span class="text-2xl font-bold tracking-tight text-[var(--sl-color-white)]">${stmtsVal}</span>
					<span class="text-xs font-semibold text-[var(--sl-color-gray-3)]">${stmtsVal === "1" ? "operation" : "operations"}</span>
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">Parsed statement count</p>
			</div>

			<!-- Card 5: Tenant Policy -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Tenant Policy</span>
					<span class="rounded px-2 py-0.5 font-mono text-[0.625rem] font-bold ${policyBadgeClass}">${policyBadge}</span>
				</div>
				<div class="mt-2.5 flex items-center gap-2 font-mono whitespace-nowrap">
					<span class="size-2 rounded-full ${policyDotClass}"></span>
					<span class="text-base font-bold ${metrics?.policyApplied ? "text-[var(--sl-color-white)]" : "text-[var(--sl-color-gray-3)]"}">${policyTitle}</span>
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">${policySub}</p>
			</div>

			<!-- Card 6: Vector Embedder -->
			<div class="rounded-xl border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] p-4 transition-all hover:border-[var(--sl-color-gray-4)]">
				<div class="flex items-center justify-between gap-2">
					<span class="font-mono text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--sl-color-gray-3)]">Vector Embedder</span>
					<span class="rounded bg-[var(--sl-color-gray-7)] px-2 py-0.5 font-mono text-[0.625rem] font-bold text-[var(--sl-color-accent-high)]">${embedBadge}</span>
				</div>
				<div class="mt-2.5 flex items-center gap-2 font-mono whitespace-nowrap">
					<span class="text-base font-bold text-[var(--sl-color-white)] truncate" title="${embedTitle}">${embedTitle}</span>
				</div>
				<p class="mt-1.5 text-xs text-[var(--sl-color-gray-3)] truncate">${embedSub}</p>
			</div>
		</div>
	`;
}

export function renderExplain(
	container: HTMLElement,
	explainText: string | null | undefined,
): void {
	if (!container) return;
	if (!explainText) {
		container.innerHTML = `
			<div class="flex h-full min-h-[14rem] flex-col items-center justify-center p-8 text-center">
				<div class="flex size-10 items-center justify-center rounded-full border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] text-[var(--sl-color-gray-3)]">
					<svg class="size-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line></svg>
				</div>
				<p class="mt-3 font-mono text-xs font-semibold uppercase tracking-wider text-[var(--sl-color-gray-2)]">Query Execution Plan</p>
				<p class="mt-1.5 max-w-xs font-mono text-xs text-[var(--sl-color-gray-3)]">
					Type a valid QQL query to inspect lowering stages, prefetch pipelines, and vector routing steps.
				</p>
			</div>
		`;
		return;
	}

	const lines = explainText.split("\n").filter((l) => l.trim().length > 0);
	const renderedSteps = lines
		.map((line, idx) => {
			const isHeader =
				line.startsWith("#") || line.toUpperCase().includes("STEP");
			return `
				<div class="flex items-start gap-3 rounded-lg border border-[var(--sl-color-gray-5)]/80 bg-[var(--sl-color-bg)]/90 p-3 font-mono text-xs transition-colors hover:border-[var(--sl-color-gray-4)]">
					<span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-[var(--sl-color-gray-7)] font-mono text-[0.625rem] font-bold text-[var(--sl-color-accent-high)]">${idx + 1}</span>
					<span class="flex-1 font-mono text-xs leading-relaxed ${isHeader ? "font-bold text-[var(--sl-color-accent-high)]" : "text-[var(--sl-color-gray-1)]"}">${escapeHtml(line)}</span>
				</div>
			`;
		})
		.join("");

	container.innerHTML = `
		<div class="flex flex-col gap-2.5">
			${renderedSteps}
		</div>
	`;
}
