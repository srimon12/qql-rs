/**
 * Command palette contents: every playground action in searchable form. The
 * palette engine lives in `palette.ts`; this module only describes commands so
 * shortcuts, menus, and the palette share one implementation per action.
 */

import { isMacPlatform, openDialog } from "../core/dom";
import { sourceText, state } from "../core/store";
import { openExporter } from "../dialogs/export";
import { loadSource } from "../editor/analysis";
import {
	copyShareLink,
	formatDocument,
	toggleCommentAtCursor,
	toggleWrap,
} from "../editor/editor-actions";
import { selectStatement } from "../editor/selection";
import { statementKeyword, statementPreview } from "../editor/statements";
import { runAll, runMode, runSmart } from "../run/run";
import type { PaletteCommand } from "./palette";

export function buildCommands(): PaletteCommand[] {
	const mac = isMacPlatform();
	const source = sourceText();
	const statementCommands: PaletteCommand[] = state.statementSpans.map(
		(span, index) => ({
			group: "Statements",
			label: `${index + 1} · ${statementKeyword(source, span)}`,
			detail: statementPreview(source, span, 70),
			run: () => selectStatement(index, { reveal: true }),
		}),
	);

	return [
		{
			group: "Run",
			label: "Run statement at cursor",
			detail: "Selection, else the statement under the caret",
			hint: mac ? "⌘↵" : "Ctrl+↵",
			run: () => void runSmart(),
		},
		{
			group: "Run",
			label: "Run selected statement",
			detail: `Run statement ${state.selectedStatement + 1} only`,
			run: () => void runMode("statement"),
		},
		{
			group: "Run",
			label: "Run all statements",
			detail: "Batch every statement in the script",
			hint: mac ? "⇧⌘↵" : "Shift+Ctrl+↵",
			run: () => void runAll(),
		},
		{
			group: "Run",
			label: "Run from current statement to end",
			run: () => void runMode("from"),
		},
		...statementCommands,
		{
			group: "Editor",
			label: "Format document",
			hint: "Alt+Shift+F",
			run: formatDocument,
		},
		{
			group: "Editor",
			label: state.wrapEnabled
				? "Disable line wrapping"
				: "Enable line wrapping",
			run: toggleWrap,
		},
		{
			group: "Editor",
			label: "Comment / uncomment selection",
			hint: mac ? "⌘/" : "Ctrl+/",
			run: toggleCommentAtCursor,
		},
		{
			group: "Editor",
			label: "Clear editor",
			run: () => loadSource(""),
		},
		{
			group: "Workspace",
			label: "Browse examples",
			run: () => openDialog("preset-dialog"),
		},
		{
			group: "Workspace",
			label: "Database & embeddings setup",
			run: () => openDialog("settings-dialog"),
		},
		{
			group: "Workspace",
			label: "Tenant policy",
			run: () => openDialog("policy-dialog"),
		},
		{ group: "Workspace", label: "Export SDK code", run: openExporter },
		{
			group: "Workspace",
			label: "Copy share link",
			run: () => void copyShareLink(),
		},
		{
			group: "Help",
			label: "Keyboard shortcuts",
			run: () => openDialog("shortcuts-dialog"),
		},
		{
			group: "Help",
			label: "Open documentation",
			run: () => window.open("/docs/", "_blank", "noopener"),
		},
	];
}
