import * as vscode from "vscode";
import { initWasm, isWasmReady } from "./core/wasm";
import { AnalysisService } from "./core/analysis";
import { createDiagnosticCollection, updateDiagnostics } from "./providers/diagnostics";
import { QqlCompletionProvider } from "./providers/completions";
import { QqlHoverProvider } from "./providers/hover";
import { QqlDocumentSymbolProvider } from "./providers/symbols";
import { QqlFoldingRangeProvider } from "./providers/folding";
import { QqlDefinitionProvider } from "./providers/definition";
import { QqlCodeLensProvider } from "./providers/codelens";
import { QqlStatusBar } from "./ui/statusBar";
import { registerCommands } from "./ui/commands";

const QQL_SELECTOR: vscode.DocumentSelector = { language: "qql" };

export function activate(context: vscode.ExtensionContext) {
  // ── Init WASM parser ──────────────────────────────────────────
  try {
    initWasm();
  } catch (err) {
    console.error("[qql-lang] WASM init failed:", err);
    vscode.window.showErrorMessage(
      `QQL: WASM parser failed to load — diagnostics disabled. ${err}`
    );
  }

  const config = vscode.workspace.getConfiguration("qql");
  const debounceMs = config.get<number>("diagnostics.debounceMs") ?? 300;

  const analysis = new AnalysisService(debounceMs);
  const diagnosticCollection = createDiagnosticCollection();
  const codeLensProvider = new QqlCodeLensProvider(analysis);
  const statusBar = new QqlStatusBar(analysis);

  // Push diagnostics whenever analysis completes
  analysis.onDidAnalyze((a) => {
    if (!a) return;
    const doc = vscode.workspace.textDocuments.find(
      (d) => d.uri.toString() === a.uri
    );
    if (doc) {
      updateDiagnostics(diagnosticCollection, doc, a.result);
    }
  });

  // ── Language providers ────────────────────────────────────────
  context.subscriptions.push(
    analysis,
    diagnosticCollection,
    statusBar,

    vscode.languages.registerCompletionItemProvider(
      QQL_SELECTOR,
      new QqlCompletionProvider(analysis),
      " ",
      ".",
      "(",
      ","
    ),
    vscode.languages.registerHoverProvider(
      QQL_SELECTOR,
      new QqlHoverProvider(analysis)
    ),
    vscode.languages.registerDocumentSymbolProvider(
      QQL_SELECTOR,
      new QqlDocumentSymbolProvider(analysis)
    ),
    vscode.languages.registerFoldingRangeProvider(
      QQL_SELECTOR,
      new QqlFoldingRangeProvider(analysis)
    ),
    vscode.languages.registerDefinitionProvider(
      QQL_SELECTOR,
      new QqlDefinitionProvider(analysis)
    ),
    vscode.languages.registerCodeLensProvider(
      QQL_SELECTOR,
      codeLensProvider
    )
  );

  // ── Commands ──────────────────────────────────────────────────
  registerCommands(context, analysis);

  // ── Document lifecycle ────────────────────────────────────────
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.languageId === "qql" && isWasmReady()) {
        analysis.analyzeNow(doc);
      }
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === "qql" && isWasmReady()) {
        analysis.analyzeNow(doc);
      }
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId === "qql" && isWasmReady()) {
        analysis.schedule(e.document);
      }
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === "qql" && isWasmReady()) {
        analysis.analyzeNow(editor.document);
      }
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      analysis.invalidate(doc.uri);
      diagnosticCollection.delete(doc.uri);
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("qql.diagnostics.debounceMs")) {
        const ms =
          vscode.workspace
            .getConfiguration("qql")
            .get<number>("diagnostics.debounceMs") ?? 300;
        analysis.setDebounceMs(ms);
      }
      if (e.affectsConfiguration("qql.codeLens.enabled")) {
        codeLensProvider.refresh();
      }
    })
  );

  // Analyze already-open QQL documents
  if (isWasmReady()) {
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId === "qql") {
        analysis.analyzeNow(doc);
      }
    }
  }
}

export function deactivate() {
  // Disposables are cleaned via context.subscriptions
}
