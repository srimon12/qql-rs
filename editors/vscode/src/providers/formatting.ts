import * as vscode from "vscode";
import { formatQql } from "../core/wasm";

/**
 * `Format Document` / `Format Selection` support for QQL via the bundled WASM
 * canonical formatter. A no-op when the document is already formatted or the
 * WASM parser is unavailable.
 */
export class QqlFormattingProvider implements vscode.DocumentFormattingEditProvider {
  provideDocumentFormattingEdits(document: vscode.TextDocument): vscode.TextEdit[] {
    const text = document.getText();
    if (!text.trim()) {
      return [];
    }
    try {
      const formatted = formatQql(text);
      if (formatted === text) {
        return [];
      }
      const fullRange = new vscode.Range(document.positionAt(0), document.positionAt(text.length));
      // The canonical output has no trailing newline; keep the file
      // newline-terminated so subsequent Format Document calls stay no-ops.
      return [vscode.TextEdit.replace(fullRange, `${formatted}\n`)];
    } catch {
      // Parse errors are surfaced by the diagnostics provider — formatting
      // must not replace the document with garbage.
      return [];
    }
  }
}
