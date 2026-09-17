import { indentWithTab } from "@codemirror/commands";
import { type Diagnostic, linter, lintGutter } from "@codemirror/lint";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { basicSetup } from "codemirror";
import type { ExecutionReport } from "qql-wasm-current";
import { formatQuery } from "qql-wasm-current";
import {
	qqlCompletion,
	qqlHighlighting,
	qqlLanguage,
} from "../playground-language";
import { formatError } from "./errors";
import { loadSession, saveSession, WORKSPACE_KEY, WRAP_KEY } from "./store";

export type { ExecutionReport };

const wrapCompartment = new Compartment();

export function isWrapOn(): boolean {
	try {
		return localStorage.getItem(WRAP_KEY) === "on";
	} catch {
		return false;
	}
}

function persistWrap(on: boolean): void {
	try {
		localStorage.setItem(WRAP_KEY, on ? "on" : "off");
	} catch {
		// Private browsing may deny storage; the toggle still works per session.
	}
}

/** `?q=` beats the session workspace, which beats the Astro default query. */
export function resolveInitialDoc(defaultQuery: string): string {
	const pageParams = new URLSearchParams(window.location.search);
	return (
		pageParams.get("q") ?? loadSession(WORKSPACE_KEY) ?? defaultQuery ?? ""
	);
}

export interface EditorHooks {
	getDiagnostics: () => Diagnostic[];
	onChange: (source: string) => void;
	onRun: () => void;
	onOpenPresets: () => void;
}

export function createEditor(
	host: HTMLElement,
	initialDoc: string,
	hooks: EditorHooks,
): EditorView {
	return new EditorView({
		parent: host,
		state: EditorState.create({
			doc: initialDoc,
			extensions: [
				basicSetup,
				qqlLanguage,
				qqlHighlighting,
				qqlCompletion,
				lintGutter(),
				linter(hooks.getDiagnostics),
				EditorView.contentAttributes.of({
					spellcheck: "false",
					autocorrect: "off",
					autocapitalize: "off",
					translate: "no",
				}),
				keymap.of([
					indentWithTab,
					{
						key: "Mod-Enter",
						run: () => {
							hooks.onRun();
							return true;
						},
					},
					{
						key: "Mod-k",
						run: () => {
							hooks.onOpenPresets();
							return true;
						},
					},
				]),
				wrapCompartment.of(isWrapOn() ? EditorView.lineWrapping : []),
				placeholder(
					"QUERY 'cozy apartment near Mitte' FROM geosmart_berlin_stays USING dense AS DENSE LIMIT 5;\n-- Nothing leaves this tab until you run.",
				),
				EditorView.updateListener.of((update) => {
					if (!update.docChanged) return;
					hooks.onChange(update.state.doc.toString());
				}),
			],
		}),
	});
}

/** Replace the whole document (presets, USING fixes, formatter). */
export function replaceDoc(editor: EditorView, text: string): void {
	editor.dispatch({
		changes: { from: 0, to: editor.state.doc.length, insert: text },
	});
}

/** Wrap toggle: CodeMirror compartment + data-wrap for the author CSS rule. */
export function setupWrapToggle(
	editor: EditorView,
	host: HTMLElement,
	toggle: HTMLButtonElement,
): void {
	const apply = (on: boolean): void => {
		editor.dispatch({
			effects: wrapCompartment.reconfigure(on ? EditorView.lineWrapping : []),
		});
		host.dataset.wrap = on ? "on" : "off";
		toggle.setAttribute("aria-pressed", String(on));
		persistWrap(on);
	};
	host.dataset.wrap = isWrapOn() ? "on" : "off";
	toggle.setAttribute("aria-pressed", String(isWrapOn()));
	toggle.addEventListener("click", () => apply(!isWrapOn()));
}

export type FormatOutcome =
	| { kind: "unchanged" }
	| { kind: "formatted"; text: string }
	| { kind: "error"; message: string };

export function tryFormat(source: string): FormatOutcome {
	try {
		const formatted = formatQuery(source);
		return formatted === source
			? { kind: "unchanged" }
			: { kind: "formatted", text: formatted };
	} catch (error) {
		return { kind: "error", message: formatError(error) };
	}
}

export function markCustomQuery(activeFixture: HTMLElement): void {
	activeFixture.textContent = "Custom query";
}

export function persistWorkspace(source: string): void {
	saveSession(WORKSPACE_KEY, source);
}
