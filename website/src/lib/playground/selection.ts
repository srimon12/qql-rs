/**
 * Statement selection: the single transition that keeps the editor caret, the
 * rail chips, and the inspector's statement navigator in agreement, plus the
 * click/change wiring for those controls.
 */
import { query } from "./dom";
import { renderCursorStatus } from "./statement-nav";
import { statementIndexAt } from "./statements";
import { editor, state } from "./store";
import { refreshInspector, refreshStatementUi } from "./ui";

export interface SelectOptions {
	/** Move the editor caret into the statement and scroll it into view. */
	reveal?: boolean;
}

/** Select statement `index` (clamped) and re-render statement-aware chrome. */
export function selectStatement(
	index: number,
	options: SelectOptions = {},
): void {
	const count = Math.max(1, state.statementSpans.length);
	state.selectedStatement = Math.min(Math.max(0, index), count - 1);
	refreshStatementUi();
	refreshInspector();
	if (options.reveal && state.statementSpans[state.selectedStatement]) {
		editor().revealStatement(state.selectedStatement);
	}
}

/**
 * Caret moved: update the readout and, when it crossed a statement boundary,
 * follow it in the rail and inspector. Same-statement moves cost one DOM write.
 */
export function followCursor(pos: number): void {
	renderCursorStatus();
	const index = statementIndexAt(state.statementSpans, pos);
	if (index >= 0 && index !== state.selectedStatement) {
		selectStatement(index);
	}
}

/**
 * Wire every statement control: the inspector select + prev/next buttons and
 * the delegated rail-chip / compiled-route-row clicks.
 */
export function setupStatementNav(): void {
	const stmtSelect = query<HTMLSelectElement>("[data-statement-select]");
	stmtSelect?.addEventListener("change", () => {
		selectStatement(Number(stmtSelect.value) || 0, { reveal: true });
	});
	query("[data-stmt-prev]")?.addEventListener("click", () => {
		selectStatement(state.selectedStatement - 1, { reveal: true });
	});
	query("[data-stmt-next]")?.addEventListener("click", () => {
		selectStatement(state.selectedStatement + 1, { reveal: true });
	});

	document.addEventListener("click", (event) => {
		const target = event.target as HTMLElement;
		const chip = target.closest<HTMLElement>("[data-stmt-chip]");
		if (chip?.dataset.stmtChip) {
			selectStatement(Number(chip.dataset.stmtChip), { reveal: true });
			return;
		}
		const routeRow = target.closest<HTMLElement>("[data-select-stmt]");
		if (routeRow?.dataset.selectStmt) {
			selectStatement(Number(routeRow.dataset.selectStmt), { reveal: true });
		}
	});
}
