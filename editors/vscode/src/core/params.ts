import * as vscode from "vscode";

/** Bind values for `:name` (object) or `?` (array) placeholders. */
export type QqlParams = Record<string, unknown> | unknown[];

/** Named param sets from the `qql.paramProfiles` setting. */
export type QqlParamProfiles = Record<string, QqlParams>;

/**
 * Read per-file bind params from a leading `-- qql-params: {...}` (or `[...]`)
 * header comment. Only the first 10 lines are scanned and the JSON must fit on
 * one line; anything else falls back to the `qql.params` setting. Returns
 * `undefined` when no header is present or it fails to parse (a malformed
 * header is ignored — the raw diagnostics will surface the bind error).
 */
export function parseParamsHeader(source: string): QqlParams | undefined {
  const head = source.split(/\r?\n/).slice(0, 10).join("\n");
  const m = head.match(/--[ \t]*qql-params\s*:\s*(.+?)\s*$/im);
  if (!m) return undefined;
  try {
    const value: unknown = JSON.parse(m[1]);
    if (value !== null && typeof value === "object") {
      return value as QqlParams;
    }
  } catch {
    // Ignore — fall through to undefined.
  }
  return undefined;
}

/**
 * Resolve bind params for a document. Precedence: the `-- qql-params:`
 * header wins, then the active named profile (`qql.activeProfile` into
 * `qql.paramProfiles`), otherwise the `qql.params` workspace setting.
 * Returns `undefined` when none is configured.
 */
export function getDocumentParams(document: vscode.TextDocument): QqlParams | undefined {
  const header = parseParamsHeader(document.getText());
  if (header !== undefined) return header;
  const profile = getActiveProfileName();
  if (profile != null) {
    const named = getParamProfiles()[profile];
    if (named !== null && typeof named === "object") return named;
  }
  const configured = vscode.workspace.getConfiguration("qql").get<QqlParams>("params");
  if (configured !== null && typeof configured === "object") {
    return configured;
  }
  return undefined;
}

/** Active profile name, or `undefined` for the default (`qql.params`). */
export function getActiveProfileName(): string | undefined {
  const name = vscode.workspace.getConfiguration("qql").get<string>("activeProfile");
  return name != null && name !== "" ? name : undefined;
}

/** Named param sets from `qql.paramProfiles` (empty when unconfigured). */
export function getParamProfiles(): QqlParamProfiles {
  const profiles = vscode.workspace.getConfiguration("qql").get<QqlParamProfiles>("paramProfiles");
  if (profiles !== null && typeof profiles === "object") return profiles;
  return {};
}

/** Display name for the currently effective params source. */
export function describeParamsSource(document: vscode.TextDocument): string | undefined {
  if (parseParamsHeader(document.getText()) !== undefined) return "header";
  const profile = getActiveProfileName();
  if (profile != null && getParamProfiles()[profile] !== undefined) return profile;
  return undefined;
}

/** Persist the active profile (`undefined` clears back to default). */
export function setActiveProfile(name: string | undefined): Thenable<void> {
  return vscode.workspace
    .getConfiguration("qql")
    .update("activeProfile", name ?? "", vscode.ConfigurationTarget.Global);
}
