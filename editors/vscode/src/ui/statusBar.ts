import * as vscode from "vscode";
import type { AnalysisService, DocumentAnalysis } from "../core/analysis";

/**
 * Status bar item showing parse state for the active QQL document.
 * Click runs qql.analyze.
 */
export class QqlStatusBar implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly analysis: AnalysisService) {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100
    );
    this.item.command = "qql.analyze";
    this.item.name = "QQL";
    this.disposables.push(this.item);

    this.disposables.push(
      analysis.onDidAnalyze((a) => {
        const active = vscode.window.activeTextEditor;
        if (!active || active.document.languageId !== "qql") return;
        if (a && a.uri === active.document.uri.toString()) {
          this.render(a);
        }
      }),
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (editor && editor.document.languageId === "qql") {
          const a = this.analysis.get(editor.document.uri);
          if (a) this.render(a);
          else {
            this.item.text = "$(sync~spin) QQL";
            this.item.tooltip = "Analyzing…";
            this.item.show();
            this.analysis.schedule(editor.document);
          }
        } else {
          this.item.hide();
        }
      })
    );
  }

  private render(a: DocumentAnalysis): void {
    if (a.result.valid) {
      const n = a.statements.length;
      this.item.text = `$(check) QQL ${n}`;
      this.item.tooltip = new vscode.MarkdownString(
        `**QQL** — valid\n\n${n} statement(s)\n\nClick to re-analyze`
      );
      this.item.backgroundColor = undefined;
    } else if (a.result.error) {
      this.item.text = `$(error) QQL`;
      this.item.tooltip = new vscode.MarkdownString(
        `**QQL** — ${a.result.error.code}\n\n${a.result.error.message}\n\nClick to re-analyze`
      );
      this.item.backgroundColor = new vscode.ThemeColor(
        "statusBarItem.errorBackground"
      );
    } else if (!a.source.trim()) {
      this.item.text = `$(file-code) QQL`;
      this.item.tooltip = "Empty QQL document";
      this.item.backgroundColor = undefined;
    } else {
      this.item.text = `$(warning) QQL`;
      this.item.tooltip = "QQL — unknown state";
      this.item.backgroundColor = undefined;
    }
    this.item.show();
  }

  dispose(): void {
    for (const d of this.disposables) d.dispose();
  }
}
