import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { compileQql, explainQql, parseQql } from "../core/wasm";
import { routeToCurl } from "../core/statements";
import type { CompiledRoute } from "../core/types";

function getActiveQqlEditor(): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "qql") {
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

async function showTextPanel(
  title: string,
  body: string,
  language: string
): Promise<void> {
  const doc = await vscode.workspace.openTextDocument({
    content: body,
    language,
  });
  await vscode.window.showTextDocument(doc, {
    preview: true,
    viewColumn: vscode.ViewColumn.Beside,
    preserveFocus: true,
  });
  // Title is not settable on untitled docs easily; first line acts as header when needed
  void title;
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
    vscode.workspace.getConfiguration("qql").get<string>("baseUrl") ??
    "http://localhost:6333";

  context.subscriptions.push(
    vscode.commands.registerCommand("qql.explain", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      const source = selectionOrDocument(editor);
      try {
        // Prefer multi-statement explain from analyze cache when full doc
        if (editor.selection.isEmpty) {
          const a = analysis.analyzeNow(editor.document);
          if (a?.result.explain) {
            await showTextPanel("QQL Explain", a.result.explain, "plaintext");
            return;
          }
        }
        const out = explainQql(source);
        await showTextPanel("QQL Explain", out, "plaintext");
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
          await showTextPanel("QQL Explain", out, "plaintext");
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
            await showTextPanel("QQL REST Route", body, "json");
            return;
          }
        }
        const source = selectionOrDocument(editor);
        const route = compileQql(source);
        await showTextPanel(
          "QQL REST Route",
          JSON.stringify(route, null, 2),
          "json"
        );
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
          await showTextPanel(
            "QQL REST Route",
            JSON.stringify(route, null, 2),
            "json"
          );
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
            // Multiple: copy all as a shell script
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
        await showTextPanel("QQL AST", JSON.stringify(ast, null, 2), "json");
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL AST failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand("qql.analyze", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      const a = analysis.analyzeNow(editor.document);
      if (!a) {
        vscode.window.showWarningMessage("QQL: analysis unavailable");
        return;
      }
      if (a.result.valid) {
        vscode.window.showInformationMessage(
          `QQL: valid · ${a.statements.length} statement(s)`
        );
      } else {
        const err = a.result.error;
        vscode.window.showErrorMessage(
          `QQL: ${err?.code ?? "error"}: ${err?.message ?? "parse failed"}`
        );
      }
    })
  );
}
