import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import type { QqlParams } from "../core/params";
import {
  getActiveProfileName,
  getDocumentParams,
  getParamProfiles,
  setActiveProfile,
} from "../core/params";
import { byteOffsetToPosition } from "../core/positions";
import { routeToCurl } from "../core/statements";
import type { CompiledRoute } from "../core/types";
import {
  analyzeQql,
  compileQql,
  explainQql,
  injectFilterQql,
  parseQql,
  tokenizeQql,
} from "../core/wasm";
import { updateDiagnostics } from "../providers/diagnostics";

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

/** Bind params for a cached statement (header wins, else `qql.params`). */
function paramsForUri(uriStr: string): QqlParams | undefined {
  const doc = vscode.workspace.textDocuments.find((d) => d.uri.toString() === uriStr);
  return doc ? safeParams(doc) : undefined;
}

function safeParams(doc: vscode.TextDocument): QqlParams | undefined {
  try {
    return getDocumentParams(doc);
  } catch {
    return undefined;
  }
}

export function registerCommands(
  context: vscode.ExtensionContext,
  analysis: AnalysisService,
  diagnosticCollection: vscode.DiagnosticCollection
): void {
  const baseUrl = () =>
    vscode.workspace.getConfiguration("qql").get<string>("baseUrl") ?? "http://localhost:6333";

  // Workspace-wide validation reports here so live typing diagnostics are undisturbed.
  const workspaceDiagnostics = vscode.languages.createDiagnosticCollection("qql-workspace");

  context.subscriptions.push(
    {
      dispose: () => {
        output?.dispose();
        output = undefined;
      },
    },
    workspaceDiagnostics,

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
        const out = explainQql(source, safeParams(editor.document));
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
          let params: QqlParams | undefined;
          if (uriStr != null && index != null) {
            source = resolveStatementSource(analysis, uriStr, index);
            params = paramsForUri(uriStr);
          }
          if (!source) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            source = selectionOrDocument(editor);
            params = safeParams(editor.document);
          }
          const out = explainQql(source, params);
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
        const route = compileQql(source, safeParams(editor.document));
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
          let params: QqlParams | undefined;

          if (uriStr != null && index != null) {
            const a = analysis.get(uriStr);
            route = a?.statements[index]?.route;
            source = a?.statements[index]?.source;
            params = paramsForUri(uriStr);
          }
          if (!route && source) {
            route = compileQql(source, params);
          }
          if (!route) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            route = compileQql(selectionOrDocument(editor), safeParams(editor.document));
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
        const params = safeParams(editor.document);
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
            route = compileQql(source, params);
          }
        } else {
          route = compileQql(source, params);
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
          let params: QqlParams | undefined;
          if (uriStr != null && index != null) {
            const a = analysis.get(uriStr);
            route = a?.statements[index]?.route;
            source = a?.statements[index]?.source;
            params = paramsForUri(uriStr);
          }
          if (!route && source) route = compileQql(source, params);
          if (!route) {
            const editor = getActiveQqlEditor();
            if (!editor) return;
            route = compileQql(selectionOrDocument(editor), safeParams(editor.document));
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
    }),

    vscode.commands.registerCommand("qql.showTokens", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        const source = selectionOrDocument(editor);
        const tokens = tokenizeQql(source) as Array<{
          kind: string;
          text: string;
          pos: number;
          end: number;
        }>;
        const head = tokens.slice(0, 500);
        const body = head.map((t) => `${t.kind} ${JSON.stringify(t.text)} @${t.pos}-${t.end}`);
        if (tokens.length > head.length) {
          body.push(`… ${tokens.length - head.length} more token(s)`);
        }
        showInOutput("QQL Tokens", body.join("\n") || "(no tokens)");
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL Tokens failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand("qql.run", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        const single = singleStatement(editor, analysis);
        if (!single) {
          vscode.window.showInformationMessage(
            "QQL: select a single statement to run (or narrow the file to one)"
          );
          return;
        }
        const route = compileQql(single, safeParams(editor.document));
        if (route.method == null || route.path == null) {
          vscode.window.showWarningMessage(
            "QQL: this statement is client-side only (no Qdrant route)"
          );
          return;
        }
        const url = `${baseUrl().replace(/\/$/, "")}${route.path}`;
        const init: RequestInit = {
          method: route.method,
          headers: { "Content-Type": "application/json" },
        };
        if (route.payload != null && route.method !== "GET" && route.method !== "DELETE") {
          init.body = JSON.stringify(route.payload);
        }
        const res = await fetch(url, init);
        const json: unknown = await res.json().catch(() => null);
        showInOutput(`QQL Run — ${route.method} ${route.path}`, renderRun(json, res));
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL Run failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand("qql.addFilter", async () => {
      const editor = getActiveQqlEditor();
      if (!editor) return;
      try {
        const field = await vscode.window.showInputBox({
          prompt: "Payload field to filter on",
          placeHolder: "tenant_id",
        });
        if (!field) return;
        const op = await vscode.window.showQuickPick(["=", ">", ">=", "<", "<="], {
          placeHolder: "Comparison operator",
        });
        if (!op) return;
        const rawValue = await vscode.window.showInputBox({
          prompt: "Filter value (JSON literal or plain string)",
          placeHolder: '"acme" or 42 or true',
        });
        if (rawValue == null) return;
        const value = parseFilterValue(rawValue);
        applyToStatements(editor, analysis, (source) => injectFilterQql(source, field, op, value));
        vscode.window.showInformationMessage(`QQL: injected ${field} ${op} ${rawValue}`);
      } catch (err) {
        vscode.window.showErrorMessage(
          `QQL Add filter failed: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }),

    vscode.commands.registerCommand("qql.validateWorkspace", async () => {
      const files = await vscode.workspace.findFiles("**/*.qql", "**/node_modules/**", 200);
      if (files.length === 0) {
        vscode.window.showInformationMessage("QQL: no .qql files in the workspace");
        return;
      }
      await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: "QQL: validating workspace" },
        async () => {
          workspaceDiagnostics.clear();
          let failed = 0;
          for (const file of files) {
            const key = file.toString();
            // Open files already report via the live collection — validate the
            // rest here without disturbing the typing flow.
            if (vscode.workspace.textDocuments.some((d) => d.uri.toString() === key)) {
              continue;
            }
            let document: vscode.TextDocument;
            try {
              // Loads without showing an editor.
              document = await vscode.workspace.openTextDocument(file);
            } catch {
              continue;
            }
            const result = analyzeQql(document.getText(), safeParams(document));
            updateDiagnostics(workspaceDiagnostics, document, result);
            // The open grabbed a live entry too — keep the single workspace one.
            diagnosticCollection.delete(file);
            if (!result.valid) failed += 1;
          }
          if (failed === 0) {
            vscode.window.showInformationMessage(
              `QQL: workspace valid · ${files.length} file(s) checked`
            );
          } else {
            vscode.window.showWarningMessage(`QQL: ${failed} file(s) with errors — see Problems`);
          }
        }
      );
    }),

    vscode.commands.registerCommand("qql.selectParamsProfile", async () => {
      const profiles = getParamProfiles();
      const names = Object.keys(profiles).sort();
      const active = getActiveProfileName();
      const items: Array<vscode.QuickPickItem & { name?: string }> = [
        {
          label: "$(check) Re-analyze document",
          description: "run qql.analyze on the active file",
        },
        {
          label: "$(circle-outline) Default (qql.params)",
          description: active == null ? "current" : undefined,
          name: "",
        },
        ...names.map((name) => ({
          label: `$(database) ${name}`,
          description: name === active ? "current" : undefined,
          name,
        })),
        { label: "$(gear) Open QQL settings", description: "edit params/profiles" },
      ];
      const picked = await vscode.window.showQuickPick(items, {
        placeHolder: "QQL params profile",
      });
      if (!picked) return;
      if (picked.label.startsWith("$(check)")) {
        await vscode.commands.executeCommand("qql.analyze");
      } else if (picked.label.startsWith("$(gear)")) {
        await vscode.commands.executeCommand(
          "workbench.action.openSettings",
          "@ext:srimon12.qql-lang qql.params"
        );
      } else {
        await setActiveProfile(picked.name !== "" ? picked.name : undefined);
        const editor = vscode.window.activeTextEditor;
        if (editor?.document.languageId === "qql") {
          analysis.analyzeNow(editor.document, { force: true });
        }
      }
    })
  );
}

/** Selection text, or the whole document when it holds exactly one statement. */
function singleStatement(editor: vscode.TextEditor, analysis: AnalysisService): string | undefined {
  if (!editor.selection.isEmpty) return editor.document.getText(editor.selection);
  const cached = analysis.get(editor.document.uri) ?? analysis.analyzeNow(editor.document);
  if (cached && cached.statements.length === 1) return cached.statements[0].source;
  return undefined;
}

/** Render a Qdrant REST response compactly: hits / count / raw JSON. */
function renderRun(json: unknown, res: Response): string {
  const status = `HTTP ${res.status}${res.ok ? "" : " (failed)"}`;
  if (json == null || typeof json !== "object") {
    return `${status}\n${JSON.stringify(json)}`;
  }
  const result = (json as { result?: unknown }).result ?? json;
  if (Array.isArray(result)) {
    const head = result.slice(0, 10);
    const lines = [`${status} · ${result.length} hit(s)`];
    if (result.length > head.length) {
      lines.push(`(showing first ${head.length})`);
    }
    lines.push(JSON.stringify(head, null, 2));
    return lines.join("\n");
  }
  if (typeof result === "object" && result !== null && "count" in result) {
    return `${status}\ncount: ${JSON.stringify((result as { count: unknown }).count)}`;
  }
  const pretty = JSON.stringify(result, null, 2);
  return `${status}\n${pretty.length > 4000 ? `${pretty.slice(0, 4000)}\n… truncated` : pretty}`;
}

/** Parse the playground value as JSON, falling back to a plain string. */
function parseFilterValue(raw: string): unknown {
  const trimmed = raw.trim();
  if (trimmed === "") return "";
  try {
    return JSON.parse(trimmed);
  } catch {
    return raw;
  }
}

/**
 * Rewrite each top-level statement (or the selection) via `rewrite`.
 * Computes every rewrite before editing so a fail-closed WASM error leaves
 * the document untouched.
 */
function applyToStatements(
  editor: vscode.TextEditor,
  analysis: AnalysisService,
  rewrite: (source: string) => string
): void {
  const document = editor.document;
  if (!editor.selection.isEmpty) {
    const source = document.getText(editor.selection);
    const out = rewrite(source);
    void editor.edit((builder) => builder.replace(editor.selection, out));
    return;
  }
  const cached = analysis.get(document.uri) ?? analysis.analyzeNow(document);
  const spans =
    cached && cached.version === document.version && cached.statements.length > 0
      ? cached.statements
      : undefined;
  if (!spans) {
    const out = rewrite(document.getText());
    const full = new vscode.Range(
      document.positionAt(0),
      document.positionAt(document.getText().length)
    );
    void editor.edit((builder) => builder.replace(full, out));
    return;
  }
  const rewritten = spans.map((span) => rewrite(span.source));
  void editor.edit((builder) => {
    spans.forEach((span, i) => {
      builder.replace(
        new vscode.Range(
          byteOffsetToPosition(document, span.start),
          byteOffsetToPosition(document, span.end)
        ),
        rewritten[i]
      );
    });
  });
}
