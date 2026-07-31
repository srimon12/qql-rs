import * as vscode from "vscode";
import type { AnalysisService, DocumentAnalysis } from "../core/analysis";

/**
 * Status bar item showing parse state for the active QQL document.
 * Click runs qql.analyze.
 *
 * Does **not** trigger analysis on its own (except the click command).
 * Lifecycle in extension.ts owns analyze/schedule.
 */
export class QqlStatusBar implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly analysis: AnalysisService) {
    this.item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    this.item.command = "qql.analyze";
    this.item.name = "QQL";
    this.disposables.push(this.item);

    this.disposables.push(
      analysis.onDidAnalyze((a) => {
        const active = vscode.window.activeTextEditor;
        if (active?.document.languageId !== "qql") return;
        if (a.uri === active.document.uri.toString()) {
          this.render(a);
        }
      }),
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (editor && editor.document.languageId === "qql") {
          const a = this.analysis.get(editor.document.uri);
          if (a) {
            this.render(a);
          } else {
            // Show idle state — extension lifecycle will analyze and notify
            this.item.text = "$(file-code) QQL";
            this.item.tooltip = "QQL — waiting for analysis";
            this.item.backgroundColor = undefined;
            this.item.show();
          }
        } else {
          this.item.hide();
        }
      })
    );

    // Initial paint if a QQL editor is already active
    const active = vscode.window.activeTextEditor;
    if (active && active.document.languageId === "qql") {
      const a = this.analysis.get(active.document.uri);
      if (a) this.render(a);
      else {
        this.item.text = "$(file-code) QQL";
        this.item.show();
      }
    }
  }

  private render(a: DocumentAnalysis): void {
    if (a.result.valid) {
      const n = a.statements.length;
      this.item.text = `$(check) QQL ${n}`;
      this.item.tooltip = `QQL — valid · ${n} statement(s)\nClick to re-analyze`;
      this.item.backgroundColor = undefined;
    } else if (a.result.error) {
      this.item.text = `$(error) QQL`;
      this.item.tooltip = `QQL — ${a.result.error.code}: ${a.result.error.message}\nClick to re-analyze`;
      this.item.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
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
