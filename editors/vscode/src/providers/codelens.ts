import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { byteOffsetToPosition } from "../core/positions";

/**
 * CodeLens above each top-level statement:
 *   Explain · Compile REST · Copy curl
 */
export class QqlCodeLensProvider implements vscode.CodeLensProvider {
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeCodeLenses = this._onDidChange.event;

  constructor(private readonly analysis: AnalysisService) {
    analysis.onDidAnalyze(() => this._onDidChange.fire());
  }

  refresh(): void {
    this._onDidChange.fire();
  }

  provideCodeLenses(
    document: vscode.TextDocument,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.CodeLens[]> {
    const config = vscode.workspace.getConfiguration("qql");
    if (config.get<boolean>("codeLens.enabled") === false) return [];

    let analysis = this.analysis.get(document.uri);
    if (!analysis || analysis.version !== document.version) {
      analysis = this.analysis.analyzeNow(document);
    }
    if (!analysis || analysis.statements.length === 0) return [];

    const lenses: vscode.CodeLens[] = [];

    for (const stmt of analysis.statements) {
      const pos = byteOffsetToPosition(document, stmt.start);
      const range = new vscode.Range(pos, pos);
      const args = [document.uri.toString(), stmt.index];

      lenses.push(
        new vscode.CodeLens(range, {
          title: `$(info) Explain`,
          command: "qql.explainStatement",
          arguments: args,
          tooltip: "Show the execution plan for this statement",
        })
      );

      if (stmt.route || analysis.result.valid) {
        lenses.push(
          new vscode.CodeLens(range, {
            title: `$(json) REST`,
            command: "qql.compileStatement",
            arguments: args,
            tooltip: "Show the compiled Qdrant REST route",
          })
        );
        lenses.push(
          new vscode.CodeLens(range, {
            title: `$(terminal) curl`,
            command: "qql.copyCurlStatement",
            arguments: args,
            tooltip: "Copy a curl command for this statement",
          })
        );
      }

      // Compact label when multi-statement
      if (analysis.statements.length > 1) {
        lenses.push(
          new vscode.CodeLens(range, {
            title: `$(symbol-method) ${stmt.kind}`,
            command: "",
            tooltip: stmt.label,
          })
        );
      }
    }

    return lenses;
  }
}
