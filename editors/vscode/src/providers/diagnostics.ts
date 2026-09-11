import * as vscode from "vscode";
import { byteOffsetToPosition } from "../core/positions";
import type { QqlToken, WasmAnalyzeResult } from "../core/types";
import { tokenizeQql } from "../core/wasm";

export type { AnalysisError, WasmAnalyzeResult } from "../core/types";

/**
 * Friendly follow-ups for error codes that otherwise read as alphabet soup.
 * Shown as a second line on the diagnostic; the `code` field keeps the exact
 * Rust code for searchability.
 */
const HINTS: Record<string, string> = {
  "QQL-PARSE-VECTOR-DIFF":
    "Hint: per-vector diffs are WITH VECTOR [name] (HNSW (…) | QUANTIZATION (…) | VECTOR (…))",
  "QQL-PLAN-VECTOR-DIFF":
    "Hint: the wire cannot express this diff (e.g. datatype) — set it at CREATE COLLECTION time",
  "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF":
    "Hint: edge only applies per-vector hnsw_config — run this statement against server Qdrant",
  "QQL-EDGE-UNSUPPORTED-SPARSE-DIFF":
    "Hint: edge has no per-sparse setter — run this statement against server Qdrant",
  "QQL-PARSE-DUPLICATE-CLAUSE":
    "Hint: clause appears twice (e.g. two WAITs) — keep a single trailing WAIT true|false",
  "QQL-UNKNOWN-VECTOR":
    "Hint: unknown vector name — check USING and the collection schema (offline: USING <name> AS MULTI)",
  "QQL-BACKEND-ENVELOPE":
    "Hint: the server returned an unexpected shape — check qql.baseUrl and the Qdrant version",
};

function hintFor(code: string): string | undefined {
  return HINTS[code] ?? (code.startsWith("QQL-BIND-") ? BIND_HINT : undefined);
}

const BIND_HINT =
  "Hint: unbound :name / ? placeholder — add a -- qql-params: {...} header or set qql.params";

/** Base URL of the per-code error reference (one anchor per code). */
const ERROR_CODES_URL = "https://qql.veristamp.in/docs/reference/error-codes";

/**
 * Create the shared diagnostic collection for QQL.
 */
export function createDiagnosticCollection(): vscode.DiagnosticCollection {
  return vscode.languages.createDiagnosticCollection("qql");
}

/**
 * Convert WASM analysis result to VS Code diagnostics and apply to the document.
 */
export function updateDiagnostics(
  collection: vscode.DiagnosticCollection,
  document: vscode.TextDocument,
  result: WasmAnalyzeResult
): void {
  const diagnostics: vscode.Diagnostic[] = [];

  if (!result.valid && result.error) {
    const err = result.error;
    let range: vscode.Range;

    if (err.start != null && err.end != null && err.end > err.start) {
      range = new vscode.Range(
        byteOffsetToPosition(document, err.start),
        byteOffsetToPosition(document, err.end)
      );
    } else if (err.start != null) {
      const pos = byteOffsetToPosition(document, err.start);
      const line = document.lineAt(pos.line);
      range = new vscode.Range(pos.line, pos.character, pos.line, line.text.length);
    } else {
      // Unspanned errors (notably QQL-BIND-*) default to the first line —
      // narrow unbound placeholders to the exact `:name` / `?` token instead.
      const refined =
        err.start == null && err.code === "QQL-BIND-MISSING-PARAM"
          ? refineBindRange(document, err.message)
          : undefined;
      if (refined) {
        range = refined;
      } else {
        const firstLine = document.lineAt(0);
        range = new vscode.Range(0, 0, 0, firstLine.text.length);
      }
    }

    const hint = hintFor(err.code);
    const diagnostic = new vscode.Diagnostic(
      range,
      hint ? `${err.code}: ${err.message}\n${hint}` : `${err.code}: ${err.message}`,
      vscode.DiagnosticSeverity.Error
    );
    diagnostic.source = "qql";
    diagnostic.code = {
      value: err.code,
      target: vscode.Uri.parse(`${ERROR_CODES_URL}/#${err.code}`),
    };
    diagnostics.push(diagnostic);

    if (result.statements_count > 0) {
      const info = new vscode.Diagnostic(
        range,
        `Parsed ${result.statements_count} statement(s) before the error`,
        vscode.DiagnosticSeverity.Information
      );
      info.source = "qql";
      diagnostics.push(info);
    }
  }

  // Soft hints on valid multi-statement scripts without trailing semicolon on last stmt
  // (no-op currently — keep diagnostics focused on real errors)

  collection.set(document.uri, diagnostics);
}

/**
 * Point a missing-param error at the exact placeholder token. `tokenize`
 * emits `:name` as COLON + IDENTIFIER; positional `?N` counts `?` tokens in
 * order. Returns `undefined` when the token cannot be found (caller keeps the
 * line fallback).
 */
function refineBindRange(document: vscode.TextDocument, message: string): vscode.Range | undefined {
  const named = message.match(/named parameter '(:[A-Za-z_][A-Za-z0-9_]*)'/)?.[1];
  const positional = message.match(/positional parameter '\?(\d+)'/)?.[1];
  if (named == null && positional == null) return undefined;
  let tokens: QqlToken[];
  try {
    tokens = tokenizeQql(document.getText()) as QqlToken[];
  } catch {
    return undefined;
  }
  if (named != null) {
    for (let i = 0; i + 1 < tokens.length; i++) {
      if (
        tokens[i].kind === "COLON" &&
        tokens[i + 1].kind === "IDENTIFIER" &&
        `:${tokens[i + 1].text}` === named
      ) {
        return new vscode.Range(
          byteOffsetToPosition(document, tokens[i].pos),
          byteOffsetToPosition(document, tokens[i + 1].end)
        );
      }
    }
    return undefined;
  }
  const want = Number(positional);
  let seen = -1;
  for (const tok of tokens) {
    if (tok.text !== "?") continue;
    seen += 1;
    if (seen === want) {
      return new vscode.Range(
        byteOffsetToPosition(document, tok.pos),
        byteOffsetToPosition(document, tok.end)
      );
    }
  }
  return undefined;
}
