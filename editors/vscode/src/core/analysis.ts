import * as vscode from "vscode";
import { analyzeQql, isWasmReady } from "./wasm";
import type { StatementSpan, WasmAnalyzeResult } from "./types";
import { extractStatementSpans } from "./statements";

export interface DocumentAnalysis {
  uri: string;
  version: number;
  source: string;
  result: WasmAnalyzeResult;
  statements: StatementSpan[];
  analyzedAt: number;
}

type AnalysisListener = (analysis: DocumentAnalysis | undefined) => void;

/**
 * Central analysis cache. One debounced analyze() per document; all providers
 * (diagnostics, codelens, symbols, status bar) share the same result.
 */
export class AnalysisService implements vscode.Disposable {
  private readonly cache = new Map<string, DocumentAnalysis>();
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
  private readonly listeners = new Set<AnalysisListener>();
  private debounceMs: number;

  constructor(debounceMs = 300) {
    this.debounceMs = debounceMs;
  }

  setDebounceMs(ms: number): void {
    this.debounceMs = Math.max(50, ms);
  }

  onDidAnalyze(listener: AnalysisListener): vscode.Disposable {
    this.listeners.add(listener);
    return new vscode.Disposable(() => this.listeners.delete(listener));
  }

  get(uri: vscode.Uri | string): DocumentAnalysis | undefined {
    const key = typeof uri === "string" ? uri : uri.toString();
    return this.cache.get(key);
  }

  /** Synchronous re-analyze (used by commands that need fresh data). */
  analyzeNow(document: vscode.TextDocument): DocumentAnalysis | undefined {
    if (document.languageId !== "qql") return undefined;
    if (!isWasmReady()) return undefined;

    const key = document.uri.toString();
    const source = document.getText();

    try {
      const result = analyzeQql(source);
      const statements = extractStatementSpans(source, result);
      const analysis: DocumentAnalysis = {
        uri: key,
        version: document.version,
        source,
        result,
        statements,
        analyzedAt: Date.now(),
      };
      this.cache.set(key, analysis);
      this.notify(analysis);
      return analysis;
    } catch (err) {
      console.error("[qql-lang] analyze error:", err);
      return this.cache.get(key);
    }
  }

  /** Debounced analyze for typing. */
  schedule(document: vscode.TextDocument): void {
    if (document.languageId !== "qql") return;
    const key = document.uri.toString();
    const existing = this.timers.get(key);
    if (existing) clearTimeout(existing);
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        this.analyzeNow(document);
      }, this.debounceMs)
    );
  }

  invalidate(uri: vscode.Uri): void {
    const key = uri.toString();
    const timer = this.timers.get(key);
    if (timer) clearTimeout(timer);
    this.timers.delete(key);
    this.cache.delete(key);
    this.notify(undefined);
  }

  private notify(analysis: DocumentAnalysis | undefined): void {
    for (const listener of this.listeners) {
      try {
        listener(analysis);
      } catch (err) {
        console.error("[qql-lang] analysis listener error:", err);
      }
    }
  }

  dispose(): void {
    for (const t of this.timers.values()) clearTimeout(t);
    this.timers.clear();
    this.cache.clear();
    this.listeners.clear();
  }
}
