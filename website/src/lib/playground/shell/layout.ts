/**
 * Workspace layout: the draggable editor/inspector split and the mobile
 * editor/result pane switch.
 */
import { query, queryAll } from "../core/dom";
import { editor, saveMobileView, state } from "../core/store";
import { SPLIT_KEY } from "../core/types";

const DEFAULT_SPLIT = 1.08;
const MIN_SPLIT = 0.6;
const MAX_SPLIT = 1.4;
/** Below this width the panes stack and the mobile switch applies. */
const DESKTOP_QUERY = "(min-width: 64rem)";

export function setupSplitHandle(workspace: HTMLElement | null): void {
	const handle = query("[data-split-handle]");
	if (!handle || !workspace) return;

	const applySplit = (ratio: number): number => {
		const clamped = Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, ratio));
		workspace.style.setProperty("--split-a", `${clamped.toFixed(3)}fr`);
		workspace.style.setProperty("--split-b", `${(2 - clamped).toFixed(3)}fr`);
		return clamped;
	};

	const ratioFromClientX = (clientX: number): number => {
		const bounds = workspace.getBoundingClientRect();
		return ((clientX - bounds.left - 4) / (bounds.width - 8)) * 2;
	};

	const saved = Number(localStorage.getItem(SPLIT_KEY));
	if (Number.isFinite(saved) && saved >= MIN_SPLIT && saved <= MAX_SPLIT) {
		applySplit(saved);
	}

	handle.addEventListener("pointerdown", (event) => {
		if (!window.matchMedia(DESKTOP_QUERY).matches) return;
		event.preventDefault();
		handle.setPointerCapture(event.pointerId);
		const onMove = (moveEvent: PointerEvent) => {
			applySplit(ratioFromClientX(moveEvent.clientX));
		};
		const onUp = (upEvent: PointerEvent) => {
			const final = applySplit(ratioFromClientX(upEvent.clientX));
			try {
				localStorage.setItem(SPLIT_KEY, String(final));
			} catch {
				// storage unavailable — the split just will not persist
			}
			editor().requestMeasure();
			handle.removeEventListener("pointermove", onMove);
			handle.removeEventListener("pointerup", onUp);
		};
		handle.addEventListener("pointermove", onMove);
		handle.addEventListener("pointerup", onUp);
	});

	handle.addEventListener("dblclick", () => {
		applySplit(DEFAULT_SPLIT);
		try {
			localStorage.setItem(SPLIT_KEY, String(DEFAULT_SPLIT));
		} catch {
			// storage unavailable — the split just will not persist
		}
		editor().requestMeasure();
	});
}

/** Reflect the persisted pane choice on the workspace and its switch buttons. */
export function applyMobileView(workspace: HTMLElement | null): void {
	if (!workspace) return;
	workspace.dataset.mobileView = state.mobileView;
	queryAll<HTMLButtonElement>("[data-mobile-view]").forEach((btn) => {
		btn.setAttribute(
			"aria-pressed",
			String(btn.dataset.mobileView === state.mobileView),
		);
	});
}

export function setupMobileViewSwitch(workspace: HTMLElement | null): void {
	queryAll<HTMLButtonElement>("[data-mobile-view]").forEach((btn) => {
		btn.addEventListener("click", () => {
			saveMobileView(btn.dataset.mobileView === "result" ? "result" : "editor");
			applyMobileView(workspace);
			editor().requestMeasure();
		});
	});
}
