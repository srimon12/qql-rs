import * as vscode from "vscode";
import { tokenizeQql } from "../core/wasm";

/**
 * QuickFixes for QQL diagnostics (read-only until invoked — no analysis):
 * remove a duplicate WAIT clause, or scaffold a `-- qql-params` entry for an
 * unbound `:name` / `?` placeholder.
 */
export class QqlCodeActionProvider implements vscode.CodeActionProvider {
  provideCodeActions(
    document: vscode.TextDocument,
    _range: vscode.Range | vscode.Selection,
    context: vscode.CodeActionContext,
    _token: vscode.CancellationToken
  ): vscode.ProviderResult<vscode.CodeAction[]> {
    const actions: vscode.CodeAction[] = [];
    for (const diag of context.diagnostics) {
      if (diag.source !== "qql") continue;
      const code = codeOf(diag);
      if (code === "QQL-PARSE-DUPLICATE-CLAUSE") {
        const fix = removeDuplicateWait(document, diag.range);
        if (fix) {
          fix.diagnostics = [diag];
          actions.push(fix);
        }
      } else if (code === "QQL-BIND-MISSING-PARAM") {
        const fix = addMissingParam(document, diag.message);
        if (fix) {
          fix.diagnostics = [diag];
          actions.push(fix);
        }
      }
    }
    return actions;
  }
}

function codeOf(diag: vscode.Diagnostic): string | undefined {
  const code = diag.code;
  if (typeof code === "string" || typeof code === "number") return String(code);
  return code?.value != null ? String(code.value) : undefined;
}

/** Delete the last `WAIT true|false` on the diagnostic line (the duplicate). */
function removeDuplicateWait(
  document: vscode.TextDocument,
  range: vscode.Range
): vscode.CodeAction | undefined {
  const lineNo = range.start.line;
  const text = document.lineAt(lineNo).text;
  const re = /\bWAIT\s+(?:true|false)\b/gi;
  let last: RegExpExecArray | undefined;
  let count = 0;
  for (let m = re.exec(text); m !== null; m = re.exec(text)) {
    last = m;
    count += 1;
  }
  if (last == null || count < 2) return undefined;
  // Swallow one run of preceding whitespace so no double space remains.
  let from = last.index;
  while (from > 0 && (text[from - 1] === " " || text[from - 1] === "\t")) from -= 1;
  const edit = new vscode.WorkspaceEdit();
  edit.replace(
    document.uri,
    new vscode.Range(lineNo, from, lineNo, last.index + last[0].length),
    ""
  );
  const action = new vscode.CodeAction(
    "Remove duplicate WAIT (keep first)",
    vscode.CodeActionKind.QuickFix
  );
  action.edit = edit;
  return action;
}

/**
 * Scaffold the missing placeholder into a `-- qql-params` header: create the
 * header when absent, otherwise merge the key (named) into it. Values start
 * as `null` so bind keeps pointing at the placeholder until filled in.
 */
function addMissingParam(
  document: vscode.TextDocument,
  message: string
): vscode.CodeAction | undefined {
  const named = message.match(/named parameter '(:[A-Za-z_][A-Za-z0-9_]*)'/)?.[1];
  const positional = message.match(/positional parameter '\?(\d+)'/) != null;
  if (named == null && !positional) return undefined;

  const text = document.getText();
  const headerLines = text.split(/\r?\n/).slice(0, 10);
  const headerIdx = headerLines.findIndex((line) => /--[ \t]*qql-params\s*:/i.test(line));
  const edit = new vscode.WorkspaceEdit();

  if (headerIdx === -1) {
    const body = named != null ? `{"${named.slice(1)}": null}` : "[]";
    edit.insert(document.uri, new vscode.Position(0, 0), `-- qql-params: ${body}\n`);
  } else if (named != null) {
    const line = headerLines[headerIdx];
    const json = line.match(/--[ \t]*qql-params\s*:\s*(.+?)\s*$/)?.[1];
    if (json == null) return undefined;
    let parsed: unknown;
    try {
      parsed = JSON.parse(json);
    } catch {
      return undefined;
    }
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      return undefined;
    }
    const key = named.slice(1);
    if (key in parsed) return undefined;
    (parsed as Record<string, unknown>)[key] = null;
    const lineStart = new vscode.Position(headerIdx, 0);
    edit.replace(
      document.uri,
      new vscode.Range(lineStart, document.lineAt(headerIdx).range.end),
      `-- qql-params: ${JSON.stringify(parsed)}`
    );
  } else {
    // Positional with an existing (underfilled) header: grow the array to the
    // placeholder count so each `?` has a slot.
    const line = headerLines[headerIdx];
    const json = line.match(/--[ \t]*qql-params\s*:\s*(.+?)\s*$/)?.[1];
    if (json == null) return undefined;
    let parsed: unknown;
    try {
      parsed = JSON.parse(json);
    } catch {
      return undefined;
    }
    if (!Array.isArray(parsed)) return undefined;
    let slots = 0;
    try {
      for (const tok of tokenizeQql(text) as Array<{ text: string }>) {
        if (tok.text === "?") slots += 1;
      }
    } catch {
      return undefined;
    }
    if (slots <= parsed.length) return undefined;
    const grown = [...parsed, ...new Array<null>(slots - parsed.length).fill(null)];
    edit.replace(
      document.uri,
      new vscode.Range(new vscode.Position(headerIdx, 0), document.lineAt(headerIdx).range.end),
      `-- qql-params: ${JSON.stringify(grown)}`
    );
  }

  const action = new vscode.CodeAction(
    named != null
      ? `Add '${named}' to qql-params (fill in the value)`
      : "Grow qql-params array for missing '?' (fill in the values)",
    vscode.CodeActionKind.QuickFix
  );
  action.edit = edit;
  return action;
}
