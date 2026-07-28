import * as vscode from "vscode";

export interface AnalysisError {
  code: string;
  message: string;
  start: number | null;
  end: number | null;
}

export interface WasmAnalyzeResult {
  valid: boolean;
  statements_count: number;
  tokens: Array<{ kind: string; text: string; pos: number; end: number; len: number }>;
  ast: unknown[] | null;
  route: unknown | null;
  routes: unknown[];
  explain: string | null;
  error: AnalysisError | null;
}

/**
 * Create the shared diagnostic collection for QQL.
 */
export function createDiagnosticCollection(): vscode.DiagnosticCollection {
  return vscode.languages.createDiagnosticCollection("qql");
}

/**
 * Convert a UTF-8 byte offset into a VS Code Position.
 *
 * The WASM parser (Rust) reports error positions as UTF-8 byte offsets.
 * VS Code's document.positionAt() expects UTF-16 code-unit offsets.
 *
 * We convert by walking the document text, counting both byte positions
 * and UTF-16 code units, then using the resulting UTF-16 offset with
 * document.positionAt() which correctly handles all line endings.
 */
function byteOffsetToPosition(document: vscode.TextDocument, byteOffset: number): vscode.Position {
  if (byteOffset <= 0) return new vscode.Position(0, 0);

  const text = document.getText();
  let utf16Offset = 0;
  let bytePos = 0;

  for (let i = 0; i < text.length && bytePos < byteOffset; i++) {
    const code = text.charCodeAt(i);

    // UTF-16 code units: 1 for BMP, 2 for surrogate pairs
    if (code >= 0xd800 && code <= 0xdfff) {
      utf16Offset += 2;
      bytePos += 4; // supplementary character → 4 UTF-8 bytes
      i++; // skip the low surrogate
    } else if (code <= 0x7f) {
      utf16Offset += 1;
      bytePos += 1;
    } else if (code <= 0x7ff) {
      utf16Offset += 1;
      bytePos += 2;
    } else {
      utf16Offset += 1;
      bytePos += 3;
    }
  }

  return document.positionAt(utf16Offset);
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
      // Fallback: mark the first line
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

    // Also add multi-statement count if applicable
    if (result.statements_count > 0) {
      const info = new vscode.Diagnostic(
        range,
        `Parsed ${result.statements_count} statement(s) — one has parse errors`,
        vscode.DiagnosticSeverity.Information
      );
      info.source = "qql";
      diagnostics.push(info);
    }
  }

  collection.set(document.uri, diagnostics);
}
