/**
 * Playground entry point. The controller is split by responsibility:
 *
 *   bootstrap.ts       composition root — initial doc, editor mount, wiring
 *   store.ts           state + localStorage; the single source of truth
 *   ui.ts              refresh hub; selection.ts keeps caret/rail/inspector in sync
 *   analysis.ts        parse + plan, debounced edit pipeline, diagnostics
 *   run.ts             run targets, execution, per-statement results, run menu
 *   editor.ts          CodeMirror setup, statement gutter, keymap
 *   inspector.ts       plan / code tabs / metrics / response panels
 *   status-bar.ts      header pills, validation badge, statement count
 *   statement-nav.ts   rail chips, cursor readout, status marks
 *   connection.ts      Qdrant client, endpoint probing, embedder config
 *   dom.ts             lookup, escaping, toasts, clipboard, dialogs
 *
 * Supporting modules: client, embedder, statements, render, export, palette,
 * editor-actions, layout, delegates, presets, settings-form, policy-form,
 * exporter, commands, shortcuts, types, wasm.
 */
import { start } from "./bootstrap";

export { showToast } from "./dom";

if (typeof window !== "undefined") {
	void start();
}
