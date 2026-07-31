import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { byteOffsetToPosition } from "../core/positions";

/**
 * Folding:
 *  1. Each multi-line top-level statement
 *  2. Parenthesized regions (CTE bodies, PREFETCH lists, VALUES objects)
 */
export class QqlFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly analysis: AnalysisService) {}

  provideFoldingRanges(
    document: vscode.TextDocument,
    _context: vscode.FoldingContext,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.FoldingRange[]> {
    const ranges: vscode.FoldingRange[] = [];
    const seen = new Set<string>();

    const add = (startLine: number, endLine: number, kind?: vscode.FoldingRangeKind) => {
      if (endLine <= startLine) return;
      const key = `${startLine}:${endLine}`;
      if (seen.has(key)) return;
      seen.add(key);
      ranges.push(new vscode.FoldingRange(startLine, endLine, kind));
    };

    // Statement-level folds
    let analysis = this.analysis.get(document.uri);
    if (!analysis || analysis.version !== document.version) {
      analysis = this.analysis.analyzeNow(document);
    }

    if (analysis) {
      for (const stmt of analysis.statements) {
        const start = byteOffsetToPosition(document, stmt.start);
        const end = byteOffsetToPosition(document, Math.max(stmt.start, stmt.end - 1));
        add(start.line, end.line);
      }

      // Paren / brace regions from tokens
      const stack: Array<{ line: number; kind: string }> = [];
      for (const tok of analysis.result.tokens) {
        const k = tok.kind.toUpperCase();
        if (k === "LPAREN" || k === "LBRACE" || k === "LBRACKET") {
          const pos = byteOffsetToPosition(document, tok.pos);
          stack.push({ line: pos.line, kind: k });
        } else if (k === "RPAREN" || k === "RBRACE" || k === "RBRACKET") {
          const open = stack.pop();
          if (open) {
            const endPos = byteOffsetToPosition(document, tok.end);
            add(open.line, endPos.line);
          }
        }
      }
    }

    // Comment block folds (consecutive -- lines)
    let commentStart: number | null = null;
    for (let line = 0; line < document.lineCount; line++) {
      const text = document.lineAt(line).text.trimStart();
      if (text.startsWith("--")) {
        if (commentStart === null) commentStart = line;
      } else {
        if (commentStart !== null && line - 1 > commentStart) {
          add(commentStart, line - 1, vscode.FoldingRangeKind.Comment);
        }
        commentStart = null;
      }
    }
    if (commentStart !== null && document.lineCount - 1 > commentStart) {
      add(commentStart, document.lineCount - 1, vscode.FoldingRangeKind.Comment);
    }

    return ranges;
  }
}
