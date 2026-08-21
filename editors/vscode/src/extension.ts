import * as vscode from "vscode";
import { AnalysisService } from "./core/analysis";
import { initWasm, isWasmReady } from "./core/wasm";
import { QqlCodeLensProvider } from "./providers/codelens";
import { QqlCompletionProvider } from "./providers/completions";
import { QqlDefinitionProvider } from "./providers/definition";
import { createDiagnosticCollection, updateDiagnostics } from "./providers/diagnostics";
import { QqlFoldingRangeProvider } from "./providers/folding";
import { QqlFormattingProvider } from "./providers/formatting";
import { QqlHoverProvider } from "./providers/hover";
import { QqlDocumentSymbolProvider } from "./providers/symbols";
import { registerCommands } from "./ui/commands";
import { QqlStatusBar } from "./ui/statusBar";

const QQL_SELECTOR: vscode.DocumentSelector = { language: "qql" };

export function activate(context: vscode.ExtensionContext) {
  try {
    initWasm();
  } catch (err) {
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

  // Diagnostics only — keep this listener cheap (no re-analyze).
  const diagnosticsSub = analysis.onDidAnalyze((a) => {
    const doc = vscode.workspace.textDocuments.find((d) => d.uri.toString() === a.uri);
    if (doc && !doc.isClosed) {
      updateDiagnostics(diagnosticCollection, doc, a.result);
    }
  });

  context.subscriptions.push(
    analysis,
    diagnosticCollection,
    codeLensProvider,
    statusBar,
    diagnosticsSub,

    vscode.languages.registerCompletionItemProvider(
      QQL_SELECTOR,
      new QqlCompletionProvider(analysis),
      " ",
      ".",
      "(",
      ","
    ),
    vscode.languages.registerHoverProvider(QQL_SELECTOR, new QqlHoverProvider(analysis)),
    vscode.languages.registerDocumentSymbolProvider(
      QQL_SELECTOR,
      new QqlDocumentSymbolProvider(analysis)
    ),
    vscode.languages.registerFoldingRangeProvider(
      QQL_SELECTOR,
      new QqlFoldingRangeProvider(analysis)
    ),
    vscode.languages.registerDefinitionProvider(QQL_SELECTOR, new QqlDefinitionProvider(analysis)),
    vscode.languages.registerCodeLensProvider(QQL_SELECTOR, codeLensProvider),
    vscode.languages.registerDocumentFormattingEditProvider(
      QQL_SELECTOR,
      new QqlFormattingProvider()
    )
  );

  registerCommands(context, analysis);

  // ── Document lifecycle (single owner of analyze/schedule) ─────
  // Providers must not drive analysis. Only these events do.
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
      // Ignore non-content events (e.g. some VS Code internal updates)
      if (e.contentChanges.length === 0) return;
      if (e.document.languageId === "qql" && isWasmReady()) {
        analysis.schedule(e.document);
      }
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === "qql" && isWasmReady()) {
        // analyzeNow is a no-op (no notify) when version is already cached
        analysis.analyzeNow(editor.document);
      }
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      if (doc.languageId !== "qql") return;
      analysis.invalidate(doc.uri);
      diagnosticCollection.delete(doc.uri);
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("qql.diagnostics.debounceMs")) {
        const ms =
          vscode.workspace.getConfiguration("qql").get<number>("diagnostics.debounceMs") ?? 300;
        analysis.setDebounceMs(ms);
      }
      if (e.affectsConfiguration("qql.codeLens.enabled")) {
        codeLensProvider.refresh();
      }
    })
  );

  // Analyze currently open QQL docs once at activation
  if (isWasmReady()) {
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.languageId === "qql") {
        analysis.analyzeNow(doc);
      }
    }
  }
}

export function deactivate() {
  // Disposables cleaned via context.subscriptions
}
