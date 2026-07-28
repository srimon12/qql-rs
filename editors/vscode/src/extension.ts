import * as vscode from "vscode";
import { initWasm, analyzeQql } from "./wasm";
import { createDiagnosticCollection, updateDiagnostics } from "./diagnostics";

let diagnosticCollection: vscode.DiagnosticCollection;
let wasmReady = false;

export function activate(context: vscode.ExtensionContext) {
  // ── Init WASM parser (synchronous, Node.js target) ─────────────
  try {
    initWasm();
    wasmReady = true;
  } catch (err) {
    console.error("[qql-lang] WASM init failed:", err);
    vscode.window.showErrorMessage(
      `QQL: WASM parser failed to load — diagnostics disabled. ${err}`
    );
  }

  // ── Diagnostics ───────────────────────────────────────────────
  diagnosticCollection = createDiagnosticCollection();

  // Per-document debounce timers, keyed by URI string
  const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function debouncedDiagnostics(document: vscode.TextDocument) {
    const key = document.uri.toString();
    const existing = debounceTimers.get(key);
    if (existing) clearTimeout(existing);
    debounceTimers.set(
      key,
      setTimeout(() => {
        debounceTimers.delete(key);
        triggerDiagnostics(document);
      }, 300)
    );
  }

  function triggerDiagnostics(document: vscode.TextDocument) {
    if (document.languageId !== "qql") return;
    if (!wasmReady) return;

    try {
      const result = analyzeQql(document.getText());
      updateDiagnostics(diagnosticCollection, document, result);
    } catch (err) {
      console.error("[qql-lang] analyze error:", err);
    }
  }

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === "qql") triggerDiagnostics(doc);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === "qql") triggerDiagnostics(doc);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId === "qql") {
        debouncedDiagnostics(e.document);
      }
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === "qql") {
        triggerDiagnostics(editor.document);
      }
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      const key = doc.uri.toString();
      const timer = debounceTimers.get(key);
      if (timer) clearTimeout(timer);
      debounceTimers.delete(key);
      diagnosticCollection.delete(doc.uri);
    }),
    diagnosticCollection
  );

  // Analyze all already-open QQL documents (foreground and background)
  for (const doc of vscode.workspace.textDocuments) {
    triggerDiagnostics(doc);
  }
}

export function deactivate() {
  if (diagnosticCollection) {
    diagnosticCollection.clear();
    diagnosticCollection.dispose();
  }
}
