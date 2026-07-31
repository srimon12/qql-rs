import * as vscode from "vscode";
import { extractStatementSpans } from "./statements";
import type { StatementSpan, WasmAnalyzeResult } from "./types";
import { analyzeQql, isWasmReady } from "./wasm";

export interface DocumentAnalysis {
  uri: string;
  version: number;
  source: string;
  result: WasmAnalyzeResult;
  statements: StatementSpan[];
  analyzedAt: number;
}

type AnalysisListener = (analysis: DocumentAnalysis) => void;

/**
 * Central analysis cache. One analyze() per document version; providers only
 * *read* the cache. Lifecycle events (open/edit/switch) own re-analysis and
 * notify listeners once — never from inside a provider callback chain.
 */
export class AnalysisService implements vscode.Disposable {
  private readonly cache = new Map<string, DocumentAnalysis>();
  private readonly timers = new Map<string, ReturnType<typeof globalThis.setTimeout>>();
  private readonly listeners = new Set<AnalysisListener>();
  private debounceMs: number;
  /** Prevent re-entrant notify storms while a listener is running. */
  private notifying = false;
  private pendingNotifications = new Map<string, DocumentAnalysis>();

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

  /**
   * Side-effect-free cache read for the current document version.
   * Returns `undefined` when missing or stale — never parses.
   * Providers should `schedule()` on a miss and refresh via `onDidAnalyze`.
   */
  ensure(document: vscode.TextDocument): DocumentAnalysis | undefined {
    if (document.languageId !== "qql") return undefined;
    const cached = this.cache.get(document.uri.toString());
    if (cached && cached.version === document.version) {
      return cached;
    }
    return undefined;
  }

  /**
   * Force analyze and notify. No-ops (no notify) when cache already matches
   * `document.version`, unless `force` is set.
   */
  analyzeNow(
    document: vscode.TextDocument,
    options?: { force?: boolean }
  ): DocumentAnalysis | undefined {
    if (document.languageId !== "qql") return undefined;
    if (!isWasmReady()) return undefined;

    const key = document.uri.toString();
    const cached = this.cache.get(key);
    if (!options?.force && cached && cached.version === document.version) {
      return cached;
    }

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

  /** Debounced analyze for typing — coalesces rapid keystrokes. */
  schedule(document: vscode.TextDocument): void {
    if (document.languageId !== "qql") return;
    const key = document.uri.toString();
    const existing = this.timers.get(key);
    if (existing) globalThis.clearTimeout(existing);
    this.timers.set(
      key,
      globalThis.setTimeout(() => {
        this.timers.delete(key);
        // Document may have closed or been disposed mid-debounce
        if (document.isClosed) return;
        this.analyzeNow(document);
      }, this.debounceMs)
    );
  }

  invalidate(uri: vscode.Uri): void {
    const key = uri.toString();
    const timer = this.timers.get(key);
    if (timer) globalThis.clearTimeout(timer);
    this.timers.delete(key);
    this.cache.delete(key);
    this.pendingNotifications.delete(key);
    // Do NOT broadcast a global "undefined" refresh — that re-queries every
    // open editor's CodeLens and can thrash the host when closing tabs.
  }

  private notify(analysis: DocumentAnalysis): void {
    // Coalesce re-entrant notifies: if a listener triggers more analysis,
    // queue the latest result per URI and flush after the stack unwinds.
    if (this.notifying) {
      this.pendingNotifications.set(analysis.uri, analysis);
      return;
    }

    this.notifying = true;
    try {
      let current: DocumentAnalysis | undefined = analysis;
      while (current) {
        const batch = current;
        current = undefined;
        for (const listener of this.listeners) {
          try {
            listener(batch);
          } catch (err) {
            console.error("[qql-lang] analysis listener error:", err);
          }
        }
        // Flush any notifies that were queued during listener execution
        if (this.pendingNotifications.size > 0) {
          // Prefer the document we just notified if it was updated again,
          // otherwise take an arbitrary pending entry.
          const queued = this.pendingNotifications.get(batch.uri);
          if (queued) {
            this.pendingNotifications.delete(batch.uri);
            current = queued;
          } else {
            const first = this.pendingNotifications.entries().next().value as
              | [string, DocumentAnalysis]
              | undefined;
            if (first) {
              this.pendingNotifications.delete(first[0]);
              current = first[1];
            }
          }
        }
      }
    } finally {
      this.notifying = false;
      this.pendingNotifications.clear();
    }
  }

  dispose(): void {
    for (const t of this.timers.values()) globalThis.clearTimeout(t);
    this.timers.clear();
    this.cache.clear();
    this.listeners.clear();
    this.pendingNotifications.clear();
  }
}
