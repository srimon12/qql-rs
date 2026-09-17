/**
 * Editor toolbar actions that mutate the buffer or the workspace: format,
 * line-wrap, comment toggle, share link.
 */
import { toggleComment } from "@codemirror/commands";
import { byId, copyToClipboard, query, queryAll, showToast } from "../core/dom";
import { editor, saveWrap, sourceText, state } from "../core/store";
import { buildFailure, formatQuery } from "../core/wasm";

export function formatDocument(): void {
	const source = sourceText();
	try {
		const formatted = formatQuery(source);
		if (formatted && formatted !== source) {
			editor().setSource(formatted);
			showToast("Query formatted");
		}
	} catch (error) {
		const failure = buildFailure(error, state.settings.qdrantUrl);
		showToast(
			failure.code ? `${failure.code}: ${failure.message}` : failure.message,
			true,
		);
	}
}

/** Apply the persisted wrap preference to the editor and its toolbar button. */
export function applyWrap(): void {
	byId("qql-editor").dataset.wrap = state.wrapEnabled ? "on" : "off";
	query("[data-wrap-toggle]")?.setAttribute(
		"aria-pressed",
		String(state.wrapEnabled),
	);
	editor().requestMeasure();
}

export function toggleWrap(): void {
	saveWrap(!state.wrapEnabled);
	applyWrap();
}

/** Palette/keyboard entry point for `Mod-/`. */
export function toggleCommentAtCursor(): void {
	if (!toggleComment(editor().view)) {
		showToast("Nothing to comment on this line", true);
	}
}

/** Copy the current script as a `?q=` link (same contract as docs links). */
export async function copyShareLink(): Promise<void> {
	const url = new URL(window.location.href);
	url.searchParams.set("q", sourceText());
	const href = url.toString();
	if (href.length > 8000) {
		showToast(
			"This script is too long to share as a link — use Export instead",
			true,
		);
		return;
	}
	await copyToClipboard(href, "Shareable link copied to clipboard");
}

export function setupEditorActions(): void {
	query("[data-format]")?.addEventListener("click", formatDocument);
	query("[data-wrap-toggle]")?.addEventListener("click", toggleWrap);
	queryAll("[data-share-link]").forEach((btn) => {
		btn.addEventListener("click", () => void copyShareLink());
	});
}
