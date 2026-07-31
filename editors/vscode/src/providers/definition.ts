import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { byteOffsetToPosition, positionToByteOffset } from "../core/positions";
import { extractCteDefinitions } from "../core/statements";

/**
 * Go-to-definition for CTE names: jump from PREFETCH (name) / references
 * back to `name AS (` in a WITH header.
 */
export class QqlDefinitionProvider implements vscode.DefinitionProvider {
  constructor(private readonly analysis: AnalysisService) {}

  provideDefinition(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.Definition> {
    const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
    if (!wordRange) return undefined;
    const name = document.getText(wordRange);

    let analysis = this.analysis.get(document.uri);
    if (!analysis || analysis.version !== document.version) {
      analysis = this.analysis.analyzeNow(document);
    }
    if (!analysis) return undefined;

    const ctes = extractCteDefinitions(analysis.source, analysis.result.tokens);
    const match = ctes.find((c) => c.name === name || c.name.toLowerCase() === name.toLowerCase());
    if (!match) return undefined;

    // Don't jump to self when already on the definition
    const offset = positionToByteOffset(document, position);
    if (offset >= match.start && offset <= match.end) return undefined;

    const start = byteOffsetToPosition(document, match.start);
    const end = byteOffsetToPosition(document, match.end);
    return new vscode.Location(document.uri, new vscode.Range(start, end));
  }
}
