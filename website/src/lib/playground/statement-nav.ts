/**
 * Statement chrome: the rail chips above the editor, the cursor readout in the
 * status strip, the run-menu statement hint, and the shared status marks used
 * by the rail, the inspector select, and the compiled-route rows.
 *
 * Render-only — selection transitions live in `selection.ts`.
 */
import { escapeHtml, query } from "./dom";
import { statementKeyword, statementPreview } from "./statements";
import { editor, sourceText, state } from "./store";
import type { StatementStatus } from "./types";

export function statusDotClass(status: StatementStatus | undefined): string {
	switch (status) {
		case "ok":
			return "bg-[var(--q-ok)]";
		case "error":
			return "bg-[var(--q-bad)]";
		case "running":
			return "bg-[var(--sl-color-accent)] animate-pulse";
		case "skipped":
			return "bg-[var(--sl-color-gray-4)]";
		default:
			return "bg-transparent ring-1 ring-inset ring-[var(--sl-color-gray-5)]";
	}
}

/** Compact `✓ / ✕ / –` suffix for `<option>` labels. */
export function statementStatusMark(
	status: StatementStatus | undefined,
): string {
	switch (status) {
		case "ok":
			return " ✓";
		case "error":
			return " ✕";
		case "skipped":
			return " –";
		default:
			return "";
	}
}

/** Dot span for the compiled-route rows. */
export function statementStatusBadge(
	status: StatementStatus | undefined,
): string {
	switch (status) {
		case "ok":
			return `<span class="size-1.5 rounded-full bg-[var(--q-ok)]" title="Ran successfully"></span>`;
		case "error":
			return `<span class="size-1.5 rounded-full bg-[var(--q-bad)]" title="Failed"></span>`;
		case "running":
			return `<span class="size-1.5 animate-pulse rounded-full bg-[var(--sl-color-accent)]" title="Running"></span>`;
		default:
			return "";
	}
}

/** One chip per statement; hidden for single-statement documents. */
export function renderStatementRail(): void {
	const rail = query("[data-statement-rail]");
	const posChip = query("[data-statement-pos]");
	const spans = state.statementSpans;
	const source = sourceText();

	if (posChip) {
		posChip.hidden = spans.length <= 1;
		posChip.textContent = `Stmt ${state.selectedStatement + 1}/${spans.length || 1}`;
	}
	if (!rail) return;
	rail.hidden = spans.length <= 1;
	if (spans.length <= 1) {
		rail.innerHTML = "";
		return;
	}
	rail.innerHTML = spans
		.map((span, index) => {
			const active = index === state.selectedStatement;
			return `<button type="button" class="app-chip h-7 shrink-0 pl-2" data-stmt-chip="${index}" aria-current="${active}" title="${escapeHtml(statementPreview(source, span, 160))}">
				<span class="font-mono text-[0.62rem] font-bold tabular-nums text-[var(--sl-color-gray-3)]">${index + 1}</span>
				<span class="font-mono text-[0.68rem] font-semibold">${escapeHtml(statementKeyword(source, span))}</span>
				<span class="size-1.5 shrink-0 rounded-full ${statusDotClass(state.statementStatus.get(index))}"></span>
			</button>`;
		})
		.join("");
}

/** `Ln 4, Col 12` readout for the editor status strip. */
export function renderCursorStatus(): void {
	const el = query("[data-cursor-pos]");
	if (!el) return;
	const pos = editor().view.state.selection.main.head;
	const doc = editor().view.state.doc;
	const line = doc.lineAt(Math.min(pos, doc.length));
	el.textContent = `Ln ${line.number}, Col ${pos - line.from + 1}`;
}

/** `Stmt 2` hint inside the run menu. */
export function renderRunMenuHint(): void {
	const hint = query("[data-run-statement-hint]");
	if (hint) hint.textContent = `Stmt ${state.selectedStatement + 1}`;
}
