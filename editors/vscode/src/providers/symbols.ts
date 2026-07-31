import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { byteOffsetToPosition } from "../core/positions";
import { extractCteDefinitions } from "../core/statements";

const KIND_MAP: Record<string, vscode.SymbolKind> = {
  QUERY: vscode.SymbolKind.Method,
  "QUERY TEXT": vscode.SymbolKind.Method,
  "QUERY HYBRID": vscode.SymbolKind.Method,
  "QUERY FUSION": vscode.SymbolKind.Method,
  "QUERY RECOMMEND": vscode.SymbolKind.Method,
  "QUERY DISCOVER": vscode.SymbolKind.Method,
  "QUERY FORMULA": vscode.SymbolKind.Method,
  "QUERY RERANK": vscode.SymbolKind.Method,
  "QUERY CROSS RERANK": vscode.SymbolKind.Method,
  "QUERY MMR": vscode.SymbolKind.Method,
  "QUERY POINTS": vscode.SymbolKind.Method,
  "QUERY ORDER BY": vscode.SymbolKind.Method,
  "QUERY SAMPLE": vscode.SymbolKind.Method,
  "QUERY IMAGE": vscode.SymbolKind.Method,
  "QUERY (CTE)": vscode.SymbolKind.Method,
  WITH: vscode.SymbolKind.Method,
  SCROLL: vscode.SymbolKind.Method,
  COUNT: vscode.SymbolKind.Number,
  UPSERT: vscode.SymbolKind.Object,
  DELETE: vscode.SymbolKind.Object,
  "DELETE PAYLOAD": vscode.SymbolKind.Object,
  "DELETE VECTOR": vscode.SymbolKind.Object,
  "CLEAR PAYLOAD": vscode.SymbolKind.Object,
  "UPDATE VECTOR": vscode.SymbolKind.Object,
  "UPDATE PAYLOAD": vscode.SymbolKind.Object,
  "CREATE COLLECTION": vscode.SymbolKind.Class,
  "CREATE INDEX": vscode.SymbolKind.Property,
  "CREATE SHARD KEY": vscode.SymbolKind.Key,
  "DROP COLLECTION": vscode.SymbolKind.Class,
  "DROP INDEX": vscode.SymbolKind.Property,
  "DROP SHARD KEY": vscode.SymbolKind.Key,
  "ALTER COLLECTION": vscode.SymbolKind.Class,
  "SHOW COLLECTIONS": vscode.SymbolKind.Namespace,
  "SHOW COLLECTION": vscode.SymbolKind.Class,
  "SHOW SHARD KEYS": vscode.SymbolKind.Key,
};

export class QqlDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly analysis: AnalysisService) {}

  provideDocumentSymbols(
    document: vscode.TextDocument,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.DocumentSymbol[]> {
    let analysis = this.analysis.get(document.uri);
    if (!analysis || analysis.version !== document.version) {
      analysis = this.analysis.analyzeNow(document);
    }
    if (!analysis) return [];

    const symbols: vscode.DocumentSymbol[] = [];

    for (const stmt of analysis.statements) {
      const range = new vscode.Range(
        byteOffsetToPosition(document, stmt.start),
        byteOffsetToPosition(document, stmt.end)
      );
      const selectionRange = range; // fine for outline
      const kind = KIND_MAP[stmt.kind] ?? vscode.SymbolKind.String;
      const detail = stmt.route
        ? `${stmt.route.method ?? "?"} ${stmt.route.path ?? ""}`.trim()
        : stmt.kind;

      const symbol = new vscode.DocumentSymbol(
        stmt.label,
        detail,
        kind,
        range,
        selectionRange
      );

      // Nested CTE children for WITH queries
      if (stmt.kind === "WITH" || stmt.kind.startsWith("QUERY") || stmt.source.match(/^\s*WITH\b/i)) {
        // Re-tokenize just this statement's tokens from full analysis by filtering offsets
        const stmtTokens = analysis.result.tokens.filter(
          (t) => t.pos >= stmt.start && t.end <= stmt.end
        );
        // Adjust token positions to absolute (already absolute)
        const ctes = extractCteDefinitions(analysis.source, stmtTokens, 0);
        for (const cte of ctes) {
          // Only CTEs that fall inside this statement
          if (cte.start < stmt.start || cte.end > stmt.end) continue;
          const cteRange = new vscode.Range(
            byteOffsetToPosition(document, cte.start),
            byteOffsetToPosition(document, cte.end)
          );
          symbol.children.push(
            new vscode.DocumentSymbol(
              cte.name,
              "CTE",
              vscode.SymbolKind.Variable,
              cteRange,
              cteRange
            )
          );
        }
      }

      symbols.push(symbol);
    }

    return symbols;
  }
}
