import * as vscode from "vscode";
import { byteOffsetToPosition } from "../core/positions";
import type { WasmAnalyzeResult } from "../core/types";

export type { AnalysisError, WasmAnalyzeResult } from "../core/types";

/**
 * Create the shared diagnostic collection for QQL.
 */
export function createDiagnosticCollection(): vscode.DiagnosticCollection {
  return vscode.languages.createDiagnosticCollection("qql");
}

/**
 * Convert WASM analysis result to VS Code diagnostics and apply to the document.
 */
export function updateDiagnostics(
  collection: vscode.DiagnosticCollection,
  document: vscode.TextDocument,
  result: WasmAnalyzeResult
): void {
  const diagnostics: vscode.Diagnostic[] = [];

  if (!result.valid && result.error) {
    const err = result.error;
    let range: vscode.Range;

    if (err.start != null && err.end != null && err.end > err.start) {
      range = new vscode.Range(
        byteOffsetToPosition(document, err.start),
        byteOffsetToPosition(document, err.end)
      );
    } else if (err.start != null) {
      const pos = byteOffsetToPosition(document, err.start);
      const line = document.lineAt(pos.line);
      range = new vscode.Range(pos.line, pos.character, pos.line, line.text.length);
    } else {
      const firstLine = document.lineAt(0);
      range = new vscode.Range(0, 0, 0, firstLine.text.length);
    }

    const diagnostic = new vscode.Diagnostic(
      range,
      `${err.code}: ${err.message}`,
      vscode.DiagnosticSeverity.Error
    );
    diagnostic.source = "qql";
    diagnostic.code = err.code;
    diagnostics.push(diagnostic);

    if (result.statements_count > 0) {
      const info = new vscode.Diagnostic(
        range,
        `Parsed ${result.statements_count} statement(s) before the error`,
        vscode.DiagnosticSeverity.Information
      );
      info.source = "qql";
      diagnostics.push(info);
    }
  }

  // Soft hints on valid multi-statement scripts without trailing semicolon on last stmt
  // (no-op currently — keep diagnostics focused on real errors)

  collection.set(document.uri, diagnostics);
}
