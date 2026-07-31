import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { routeToCurl } from "../core/statements";
import type { CompiledRoute } from "../core/types";
import { compileQql, explainQql, parseQql } from "../core/wasm";

/** Single reusable output channel — avoids opening dozens of untitled docs. */
let output: vscode.OutputChannel | undefined;

function getOutput(): vscode.OutputChannel {
  if (!output) {
    output = vscode.window.createOutputChannel("QQL");
  }
  return output;
}

function showInOutput(title: string, body: string): void {
  const ch = getOutput();
  ch.clear();
  ch.appendLine(`── ${title} ──`);
  ch.appendLine("");
  ch.appendLine(body);
  ch.show(true);
}

function getActiveQqlEditor(): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor;
  if (editor?.document.languageId !== "qql") {
    vscode.window.showWarningMessage("QQL: open a .qql file first");
    return undefined;
  }
  return editor;
}

function selectionOrDocument(editor: vscode.TextEditor): string {
  const sel = editor.selection;
  if (!sel.isEmpty) {
    return editor.document.getText(sel);
  }
  return editor.document.getText();
}

function resolveStatementSource(
  analysis: AnalysisService,
  uriStr: string,
  index: number
): string | undefined {
  const analysisResult = analysis.get(uriStr);
  if (!analysisResult) return undefined;
  const stmt = analysisResult.statements[index];
  return stmt?.source;
}

export function registerCommands(
  context: vscode.ExtensionContext,
  analysis: AnalysisService
): void {
  const baseUrl = () =>
    vscode.workspace.getConfiguration("qql").get<string>("baseUrl") ?? "http://localhost:6333";

  context.subscriptions.push(
    {
      dispose: () => {
        output?.dispose();
        output = undefined;
      },
    },

    // No-op used by label-only CodeLens entries
    vscode.commands.registerCommand("qql.noop", () => undefined),

    vscode.commands.registerCommand("qql.explain", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      const source = selectionOrDocument(editor);
      try {
        if (editor.selection.isEmpty) {
          // User-invoked: analyzeNow is OK (not a provider path)
          const a = analysis.analyzeNow(editor.document);
          if (a?.result.explain) {
            showInOutput("QQL Explain", a.result.explain);
            return;
          }
        }
        const out = explainQql(source);
        showInOutput("QQL Explain", out);
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL Explain failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand(
      "qql.explainStatement",
      async (uriStr?: string, index?: number) => {
        try {
          let source: string | undefined;
          if (uriStr != null && index != null) {
            source = resolveStatementSource(analysis, uriStr, index);
          }
          if (!source) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            source = selectionOrDocument(editor);
          }
          const out = explainQql(source);
          showInOutput("QQL Explain", out);
        } catch (err) {
          vscode.window.showErrorMessage(
            `QQL Explain failed: ${err instanceof Error ? err.message : String(err)}`
          );
        }
      }
    ),

    vscode.commands.registerCommand("qql.compile", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        if (editor.selection.isEmpty) {
          const a = analysis.analyzeNow(editor.document);
          if (a?.result.routes?.length) {
            const body = JSON.stringify(
              a.result.routes.length === 1 ? a.result.routes[0] : a.result.routes,
              null,
              2
            );
            showInOutput("QQL REST Route", body);
            return;
          }
        }
        const source = selectionOrDocument(editor);
        const route = compileQql(source);
        showInOutput("QQL REST Route", JSON.stringify(route, null, 2));
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL Compile failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand(
      "qql.compileStatement",
      async (uriStr?: string, index?: number) => {
        try {
          let route: CompiledRoute | undefined;
          let source: string | undefined;

          if (uriStr != null && index != null) {
            const a = analysis.get(uriStr);
            route = a?.statements[index]?.route;
            source = a?.statements[index]?.source;
          }
          if (!route && source) {
            route = compileQql(source);
          }
          if (!route) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            route = compileQql(selectionOrDocument(editor));
          }
          showInOutput("QQL REST Route", JSON.stringify(route, null, 2));
        } catch (err) {
          vscode.window.showErrorMessage(
            `QQL Compile failed: ${err instanceof Error ? err.message : String(err)}`
          );
        }
      }
    ),

    vscode.commands.registerCommand("qql.copyCurl", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        const source = selectionOrDocument(editor);
        let route: CompiledRoute;
        if (editor.selection.isEmpty) {
          const a = analysis.analyzeNow(editor.document);
          if (a?.result.routes?.length === 1) {
            route = a.result.routes[0];
          } else if (a?.result.routes && a.result.routes.length > 1) {
            const script = a.result.routes
              .map((r, i) => `# Statement ${i + 1}\n${routeToCurl(r, baseUrl())}`)
              .join("\n\n");
            await vscode.env.clipboard.writeText(script);
            vscode.window.showInformationMessage(
              `QQL: copied ${a.result.routes.length} curl commands`
            );
            return;
          } else {
            route = compileQql(source);
          }
        } else {
          route = compileQql(source);
        }
        const curl = routeToCurl(route, baseUrl());
        await vscode.env.clipboard.writeText(curl);
        vscode.window.showInformationMessage("QQL: curl command copied");
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL curl failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand(
      "qql.copyCurlStatement",
      async (uriStr?: string, index?: number) => {
        try {
          let route: CompiledRoute | undefined;
          let source: string | undefined;
          if (uriStr != null && index != null) {
            const a = analysis.get(uriStr);
            route = a?.statements[index]?.route;
            source = a?.statements[index]?.source;
          }
          if (!route && source) route = compileQql(source);
          if (!route) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            route = compileQql(selectionOrDocument(editor));
          }
          const curl = routeToCurl(route, baseUrl());
          await vscode.env.clipboard.writeText(curl);
          vscode.window.showInformationMessage("QQL: curl command copied");
        } catch (err) {
          vscode.window.showErrorMessage(
            `QQL curl failed: ${err instanceof Error ? err.message : String(err)}`
          );
        }
      }
    ),

    vscode.commands.registerCommand("qql.showAst", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        const source = selectionOrDocument(editor);
        let ast: unknown;
        if (editor.selection.isEmpty) {
          const a = analysis.analyzeNow(editor.document);
          ast = a?.result.ast ?? parseQql(source);
        } else {
          ast = parseQql(source);
        }
        showInOutput("QQL AST", JSON.stringify(ast, null, 2));
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL AST failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand("qql.analyze", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      const a = analysis.analyzeNow(editor.document, { force: true });
      if (!a) {
        vscode.window.showWarningMessage("QQL: analysis unavailable");
        return;
      }
      if (a.result.valid) {
        vscode.window.showInformationMessage(`QQL: valid · ${a.statements.length} statement(s)`);
      } else {
        const err = a.result.error;
        vscode.window.showErrorMessage(
          `QQL: ${err?.code ?? "error"}: ${err?.message ?? "parse failed"}`
        );
      }
    })
  );
}
