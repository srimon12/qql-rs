/**
 * CodeMirror playground editor: language wiring, statement awareness, and
 * the editing keymap. Execution lives in `playground.ts`; this module only
 * reports intent through callbacks.
 */
import { indentWithTab, toggleComment } from "@codemirror/commands";
import { type Diagnostic, linter, lintGutter } from "@codemirror/lint";
import {
	EditorState,
	type Extension,
	RangeSetBuilder,
	StateField,
} from "@codemirror/state";
import {
	Decoration,
	type DecorationSet,
	EditorView,
	GutterMarker,
	gutter,
	keymap,
	placeholder,
	ViewPlugin,
	type ViewUpdate,
} from "@codemirror/view";
import { basicSetup } from "codemirror";
import type { StatementSpan } from "../core/types";
import {
	qqlCommentTokens,
	qqlCompletion,
	qqlHighlighting,
	qqlLanguage,
} from "./language";
import {
	scanStatementSpans,
	statementIndexAt,
	statementStartOffset,
} from "./statements";

export type RunShortcut = "smart" | "all";

export interface PlaygroundEditorOptions {
	parent: HTMLElement;
	doc: string;
	placeholderText?: string;
	diagnostics: () => Diagnostic[];
	onDocChange: (source: string) => void;
	onCursor: (pos: number) => void;
	onRun: (shortcut: RunShortcut) => void;
	onFormat: () => void;
	onSelectStatement: (index: number) => void;
}

export interface PlaygroundEditor {
	readonly view: EditorView;
	spans(): StatementSpan[];
	source(): string;
	setSource(source: string, options?: { anchor?: number }): void;
	selectionText(): string | null;
	cursorStatement(): number;
	revealStatement(index: number, focus?: boolean): void;
	requestMeasure(): void;
}

/** Statement spans recomputed only when the document changes. */
export const statementSpansField = StateField.define<StatementSpan[]>({
	create: (state) => scanStatementSpans(state.doc.toString()),
	update: (value, tr) =>
		tr.docChanged ? scanStatementSpans(tr.state.doc.toString()) : value,
});

function activeStatementDecorations(state: EditorState): DecorationSet {
	const spans = state.field(statementSpansField, false);
	if (!spans || spans.length === 0) return Decoration.none;
	const index = statementIndexAt(spans, state.selection.main.head);
	if (index < 0) return Decoration.none;
	const span = spans[index];
	const doc = state.doc;
	const first = doc.lineAt(Math.min(span.start, doc.length));
	const last = doc.lineAt(
		Math.min(Math.max(span.end - 1, span.start), doc.length),
	);
	const builder = new RangeSetBuilder<Decoration>();
	for (let line = first.number; line <= last.number; line += 1) {
		const info = doc.line(line);
		builder.add(
			info.from,
			info.from,
			Decoration.line({
				class:
					line === first.number
						? "cm-stmt-active cm-stmt-start"
						: "cm-stmt-active",
			}),
		);
	}
	return builder.finish();
}

const activeStatementTint = ViewPlugin.fromClass(
	class {
		decorations: DecorationSet;
		constructor(view: EditorView) {
			this.decorations = activeStatementDecorations(view.state);
		}
		update(update: ViewUpdate) {
			if (update.docChanged || update.selectionSet || update.viewportChanged) {
				this.decorations = activeStatementDecorations(update.state);
			}
		}
	},
	{ decorations: (plugin) => plugin.decorations },
);

class StatementGutterMarker extends GutterMarker {
	constructor(readonly index: number) {
		super();
	}
	override elementClass = "cm-stmt-marker-line";
	/**
	 * Fresh DOM per insertion: CodeMirror owns gutter elements and may insert
	 * the same marker into a rebuilt gutter element, so a cached node would be
	 * moved out from under the previous one.
	 */
	override toDOM(): HTMLElement {
		const marker = document.createElement("span");
		marker.className = "cm-stmt-marker";
		marker.textContent = String(this.index + 1);
		return marker;
	}
	override eq(other: GutterMarker): boolean {
		return other instanceof StatementGutterMarker && other.index === this.index;
	}
}

// Stable instances keep RangeSet.eq cheap: unchanged statements skip re-renders.
const markerCache = new Map<number, StatementGutterMarker>();
function statementMarker(index: number): StatementGutterMarker {
	let marker = markerCache.get(index);
	if (!marker) {
		marker = new StatementGutterMarker(index);
		markerCache.set(index, marker);
	}
	return marker;
}

function statementGutter(onSelectStatement: (index: number) => void) {
	return gutter({
		class: "cm-stmt-gutter",
		markers: (view) => {
			const spans = view.state.field(statementSpansField);
			const builder = new RangeSetBuilder<GutterMarker>();
			for (const [index, span] of spans.entries()) {
				const line = view.state.doc.lineAt(
					Math.min(span.start, view.state.doc.length),
				);
				builder.add(line.from, line.from, statementMarker(index));
			}
			return builder.finish();
		},
		domEventHandlers: {
			click(view, line) {
				const spans = view.state.field(statementSpansField);
				const lineNumber = view.state.doc.lineAt(
					Math.min(line.from, view.state.doc.length),
				).number;
				const index = spans.findIndex(
					(span) =>
						view.state.doc.lineAt(Math.min(span.start, view.state.doc.length))
							.number === lineNumber,
				);
				if (index < 0) return false;
				onSelectStatement(index);
				return true;
			},
		},
	});
}

export function createPlaygroundEditor(
	options: PlaygroundEditorOptions,
): PlaygroundEditor {
	const {
		parent,
		doc,
		diagnostics,
		onDocChange,
		onCursor,
		onRun,
		onFormat,
		onSelectStatement,
	} = options;

	const extensions: Extension[] = [
		statementSpansField,
		statementGutter(onSelectStatement),
		activeStatementTint,
		// Before `basicSetup`: its defaultKeymap binds Mod-Enter (blank line) and
		// Mod-/ (toggleComment), and earlier extensions take precedence.
		keymap.of([
			indentWithTab,
			{
				key: "Mod-Enter",
				run: () => {
					onRun("smart");
					return true;
				},
			},
			{
				key: "Shift-Mod-Enter",
				run: () => {
					onRun("all");
					return true;
				},
			},
			{ key: "Mod-/", run: toggleComment },
			{
				key: "Alt-Shift-f",
				run: () => {
					onFormat();
					return true;
				},
			},
		]),
		basicSetup,
		qqlLanguage,
		qqlCommentTokens,
		qqlCompletion,
		qqlHighlighting,
		lintGutter(),
		linter(() => diagnostics()),
		placeholder(
			options.placeholderText ?? "-- Type your QQL vector query here…",
		),
		EditorView.updateListener.of((update) => {
			if (update.docChanged) onDocChange(update.state.doc.toString());
			if (update.selectionSet || update.docChanged) {
				onCursor(update.state.selection.main.head);
			}
		}),
	];

	const view = new EditorView({
		state: EditorState.create({ doc, extensions }),
		parent,
	});

	return {
		view,
		spans: () => view.state.field(statementSpansField),
		source: () => view.state.doc.toString(),
		setSource: (source: string, options?: { anchor?: number }) => {
			const anchor =
				options?.anchor !== undefined
					? Math.min(options.anchor, source.length)
					: Math.min(view.state.selection.main.anchor, source.length);
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: source },
				selection: { anchor },
			});
		},
		selectionText: () => {
			const range = view.state.selection.main;
			if (range.empty) return null;
			const text = view.state.sliceDoc(range.from, range.to).trim();
			return text.length > 0 ? text : null;
		},
		cursorStatement: () =>
			statementIndexAt(
				view.state.field(statementSpansField),
				view.state.selection.main.head,
			),
		revealStatement: (index: number, focus = true) => {
			const spans = view.state.field(statementSpansField);
			const span = spans[index];
			if (!span) return;
			const anchor = Math.min(
				statementStartOffset(view.state.doc.toString(), span),
				view.state.doc.length,
			);
			view.dispatch({
				selection: { anchor },
				effects: EditorView.scrollIntoView(anchor, { y: "center" }),
			});
			if (focus) view.focus();
		},
		requestMeasure: () => view.requestMeasure(),
	};
}
