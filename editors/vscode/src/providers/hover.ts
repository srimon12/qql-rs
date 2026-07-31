import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { positionToByteOffset } from "../core/positions";
import { statementAtOffset } from "../core/statements";
import { formatKeywordHover, lookupKeywordDoc } from "../data/docs";

export class QqlHoverProvider implements vscode.HoverProvider {
  constructor(private readonly analysis: AnalysisService) {}

  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.Hover> {
    // Prefer word under cursor for keyword docs
    const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
    const word = wordRange ? document.getText(wordRange) : "";

    const parts: vscode.MarkdownString[] = [];

    if (word) {
      const doc = lookupKeywordDoc(word);
      if (doc) {
        const md = new vscode.MarkdownString(formatKeywordHover(doc));
        md.supportHtml = false;
        md.isTrusted = false;
        parts.push(md);
      }
    }

    // Attach live explain for the enclosing statement when analysis is ready
    const cached = this.analysis.get(document.uri);
    if (cached?.result.valid && cached.statements.length > 0) {
      const offset = positionToByteOffset(document, position);
      const stmt = statementAtOffset(cached.statements, offset);
      if (stmt) {
        // Pull the matching section from multi-statement explain
        const explain = sectionForStatement(cached.result.explain, stmt.index);
        if (explain) {
          const md = new vscode.MarkdownString();
          md.appendMarkdown(`### Statement plan\n`);
          md.appendCodeblock(explain.trim(), "text");
          if (stmt.route?.method && stmt.route?.path) {
            md.appendMarkdown(
              `\n**REST** \`${stmt.route.method} ${stmt.route.path}\` · _${stmt.route.stmt_type}_`
            );
          }
          parts.push(md);
        }
      }
    }

    if (parts.length === 0) return undefined;
    return new vscode.Hover(parts, wordRange);
  }
}

function sectionForStatement(explain: string | null, index: number): string | null {
  if (!explain) return null;
  if (!explain.includes("--- Statement")) {
    return index === 0 ? explain : null;
  }
  const sections = explain.split(/--- Statement \d+ ---\n/);
  // split yields leading empty string before first header
  const body = sections[index + 1] ?? sections[index];
  return body?.trim() ? body : null;
}
