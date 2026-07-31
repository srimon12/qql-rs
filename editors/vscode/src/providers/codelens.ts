import * as vscode from "vscode";
import type { AnalysisService, DocumentAnalysis } from "../core/analysis";
import { byteOffsetToPosition } from "../core/positions";

/**
 * CodeLens above each top-level statement:
 *   Explain · REST · curl
 *
 * Important: this provider is **read-only** against the analysis cache.
 * It never calls analyzeNow() — that would notify listeners, fire
 * onDidChangeCodeLenses, and re-enter provideCodeLenses (host thrash / Git hang).
 */
export class QqlCodeLensProvider implements vscode.CodeLensProvider, vscode.Disposable {
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeCodeLenses = this._onDidChange.event;

  private readonly disposables: vscode.Disposable[] = [];
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;

  constructor(private readonly analysis: AnalysisService) {
    // Debounce refresh so a burst of analyses (tab switch + open + save)
    // collapses into one CodeLens pass.
    this.disposables.push(
      analysis.onDidAnalyze(() => this.scheduleRefresh()),
      this._onDidChange
    );
  }

  refresh(): void {
    this.scheduleRefresh();
  }

  private scheduleRefresh(): void {
    if (this.disposed) return;
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = undefined;
      if (!this.disposed) {
        this._onDidChange.fire();
      }
    }, 50);
  }

  provideCodeLenses(
    document: vscode.TextDocument,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.CodeLens[]> {
    const config = vscode.workspace.getConfiguration("qql");
    if (config.get<boolean>("codeLens.enabled") === false) return [];

    // Cache only — if stale/missing, lifecycle handlers will analyze and
    // this provider will refresh via onDidAnalyze.
    const analysis = this.analysis.get(document.uri);
    if (!analysis) {
      // Kick a debounced analyze without blocking/re-entering this callback.
      this.analysis.schedule(document);
      return [];
    }
    if (analysis.version !== document.version) {
      this.analysis.schedule(document);
      // Show lenses from slightly stale analysis rather than blanking out.
    }

    return this.buildLenses(document, analysis);
  }

  private buildLenses(
    document: vscode.TextDocument,
    analysis: DocumentAnalysis
  ): vscode.CodeLens[] {
    if (analysis.statements.length === 0) return [];

    const lenses: vscode.CodeLens[] = [];

    for (const stmt of analysis.statements) {
      const pos = byteOffsetToPosition(document, stmt.start);
      const range = new vscode.Range(pos, pos);
      const args = [document.uri.toString(), stmt.index];

      lenses.push(
        new vscode.CodeLens(range, {
          title: "$(info) Explain",
          command: "qql.explainStatement",
          arguments: args,
          tooltip: "Show the execution plan for this statement",
        })
      );

      if (stmt.route || analysis.result.valid) {
        lenses.push(
          new vscode.CodeLens(range, {
            title: "$(json) REST",
            command: "qql.compileStatement",
            arguments: args,
            tooltip: "Show the compiled Qdrant REST route",
          })
        );
        lenses.push(
          new vscode.CodeLens(range, {
            title: "$(terminal) curl",
            command: "qql.copyCurlStatement",
            arguments: args,
            tooltip: "Copy a curl command for this statement",
          })
        );
      }

      if (analysis.statements.length > 1) {
        // Label-only lens: command is a harmless no-op (VS Code requires a command id)
        lenses.push(
          new vscode.CodeLens(range, {
            title: `$(symbol-method) ${stmt.kind}`,
            command: "qql.noop",
            tooltip: stmt.label,
          })
        );
      }
    }

    return lenses;
  }

  dispose(): void {
    this.disposed = true;
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    for (const d of this.disposables) d.dispose();
  }
}
