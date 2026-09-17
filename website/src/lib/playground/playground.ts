/**
 * Playground entry point.
 *
 * ```
 * playground/
 * ├── playground.ts   this entry — mounts and starts
 * ├── bootstrap.ts    composition root: initial document, editor, wiring
 * ├── core/           store, types, DOM plumbing, WASM bridge, refresh hub
 * ├── editor/         CodeMirror, language, statements, analysis, selection
 * ├── run/            run targets, execution, per-statement results, run menu
 * ├── panels/         inspector, status bar, statement chrome, response renderers
 * ├── dialogs/        presets, settings, policy, export
 * ├── shell/          workspace layout, palette, commands, shortcuts, delegates
 * └── services/       Qdrant client, connection, embedder, SDK codegen
 * ```
 */
import { start } from "./bootstrap";

export { showToast } from "./core/dom";

if (typeof window !== "undefined") {
	void start();
}
