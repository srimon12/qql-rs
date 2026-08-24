import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import {
  forceLinting,
  linter,
  lintGutter,
  type Diagnostic,
} from "@codemirror/lint";
import initQql, {
  analyze,
  Client,
  Stmt,
  type AnalysisResult,
  type CompiledRoute,
} from "qql-wasm-current";
import { createBrowserEmbedder } from "./browser-embedder";
import {
  qqlCompletion,
  qqlHighlighting,
  qqlLanguage,
} from "./playground-language";
import {
  DEFAULT_POLICY,
  DEFAULT_SETTINGS,
  type ExportLanguage,
  type InspectorTab,
  type PlaygroundAnalysis,
  type PlaygroundSettings,
  type PlaygroundState,
  type PolicyValueType,
  type RuntimePolicy,
} from "./playground-types";

const SETTINGS_KEY = "qql-playground.settings.v1";
const POLICY_KEY = "qql-playground.policy.v1";
const WORKSPACE_KEY = "qql-playground.workspace.v1";
const INSPECTOR_TAB_KEY = "qql-playground.inspector-tab.v1";
const ANALYSIS_DELAY_MS = 90;

function required<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Playground element is missing: ${selector}`);
  return element;
}

function all<T extends Element>(selector: string): T[] {
  return [...document.querySelectorAll<T>(selector)];
}

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function loadStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return { ...fallback };
    return { ...fallback, ...(JSON.parse(raw) as Partial<T>) };
  } catch {
    return { ...fallback };
  }
}

function loadSession(key: string): string | null {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function saveSession(key: string, value: string): void {
  try {
    sessionStorage.setItem(key, value);
  } catch {
    // Private browsing may deny storage; the playground remains usable.
  }
}

function policyValue(policy: RuntimePolicy): string | number | boolean {
  if (policy.valueType === "number") {
    const number = Number(policy.value);
    if (!Number.isFinite(number)) throw new Error("Policy value must be a number.");
    return number;
  }
  if (policy.valueType === "boolean") {
    if (policy.value !== "true" && policy.value !== "false") {
      throw new Error("Boolean policy values must be true or false.");
    }
    return policy.value === "true";
  }
  return policy.value;
}

function byteOffsetToPosition(source: string, offset: number): number {
  const bytes = new TextEncoder().encode(source);
  const safeOffset = Math.max(0, Math.min(offset, bytes.length));
  return new TextDecoder().decode(bytes.slice(0, safeOffset)).length;
}

function quotePython(source: string): string {
  return `qql = """${source.replace(/"""/g, '\\\"\\\"\\\"')}"""`;
}

function quoteRust(source: string): string {
  const hashes = source.includes('"#') ? "##" : "#";
  return `r${hashes}"${source}"${hashes}`;
}

function exportCode(
  language: ExportLanguage,
  source: string,
  settings: PlaygroundSettings,
  route: CompiledRoute | null,
  statementCount: number,
): string {
  const url = JSON.stringify(settings.qdrantUrl);
  const pythonKey = settings.qdrantKey ? JSON.stringify(settings.qdrantKey) : "None";

  if (language === "python") {
    return `# pip install pyqql
from pyqql import Client

${quotePython(source)}
# execute() accepts a complete QQL script, including multiple statements.
client = Client(url=${url}, api_key=${pythonKey})
report = client.execute(qql)
print(report)`;
  }

  if (language === "node") {
    const apiKey = settings.qdrantKey
      ? `,\n  apiKey: ${JSON.stringify(settings.qdrantKey)}`
      : "";
    return `// npm install @veristamp/nqql
import { Client } from "@veristamp/nqql";

const qql = ${JSON.stringify(source)};
const client = new Client({
  url: ${url}${apiKey}
});
const report = await client.execute(qql);
console.log(report);`;
  }

  if (language === "rust") {
    const rustKey = settings.qdrantKey
      ? `Some(${JSON.stringify(settings.qdrantKey)}.to_owned())`
      : "None";
    return `// Cargo.toml: qql = "0.1", tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executor = Executor::rest(${url}, ${rustKey})?;
    let report = executor
        .execute(${quoteRust(source)}, OnError::Stop)
        .await?;
    println!("{report:#?}");
    Ok(())
}`;
  }

  if (!route) return "A compiled route is required before exporting cURL.";
  const payload = pretty(route.payload).replace(/'/g, "'\\''");
  const header = settings.qdrantKey
    ? ` \\\n  -H ${JSON.stringify(`api-key: ${settings.qdrantKey}`)}`
    : "";
  return `# Statement ${state.selectedStatement + 1} of ${statementCount} · compiled from the editor
curl -X ${route.method} ${JSON.stringify(`${settings.qdrantUrl}${route.path}`)} \\
  -H "content-type: application/json"${header} \\
  --data '${payload}'`;
}

const workspace = required<HTMLElement>("[data-default-query]");
const editorHost = required<HTMLElement>("#qql-editor");
const runButton = required<HTMLButtonElement>("[data-run]");
const exportButton = required<HTMLButtonElement>("[data-open-export]");
const validationBadge = required<HTMLElement>("[data-validation-badge]");
const analysisSummary = required<HTMLElement>("[data-analysis-summary]");
const runtimeStatus = required<HTMLElement>("[data-runtime-status]");
const connectionSummary = required<HTMLElement>("[data-connection-summary]");
const activeFixture = required<HTMLElement>("[data-active-fixture]");
const statementSelect = required<HTMLSelectElement>("[data-statement-select]");
const planEmpty = required<HTMLElement>("[data-empty-plan]");
const planCard = required<HTMLElement>("[data-plan-card]");
const routeMethod = required<HTMLElement>("[data-route-method]");
const routePath = required<HTMLElement>("[data-route-path]");
const routeType = required<HTMLElement>("[data-route-type]");
const routePolicy = required<HTMLElement>("[data-route-policy]");
const routesSection = required<HTMLElement>("[data-routes-section]");
const routesList = required<HTMLElement>("[data-routes-list]");
const routesCount = required<HTMLElement>("[data-routes-count]");
const policyDot = required<HTMLElement>("[data-policy-dot]");
const toastRegion = required<HTMLElement>("[data-toast-region]");
const embedStatus = required<HTMLElement>("[data-embed-status]");
const editorLoading = required<HTMLElement>("[data-editor-loading]");
const shareButton = required<HTMLButtonElement>("[data-share]");
const docsBacklink = required<HTMLAnchorElement>("[data-docs-backlink]");

const state: PlaygroundState = {
  analysis: null,
  response: null,
  executionError: null,
  selectedStatement: 0,
  inspectorTab: "plan",
  exportLanguage: "python",
  metrics: null,
};

let settings = loadStored(SETTINGS_KEY, DEFAULT_SETTINGS);
let policy = loadStored(POLICY_KEY, DEFAULT_POLICY);
let client: Client | null = null;
let activeExecutions = 0;
const retiredClients = new Set<Client>();
let analysisTimer: number | null = null;

function toast(message: string, tone: "success" | "error" = "success"): void {
  const item = document.createElement("div");
  item.className = `toast toast--${tone}`;
  item.textContent = message;
  toastRegion.append(item);
  window.setTimeout(() => item.remove(), 3600);
}

function setRuntime(message: string): void {
  runtimeStatus.textContent = message;
}

function setEditorLoading(message: string | null): void {
  editorLoading.hidden = message == null;
  if (message != null) editorLoading.textContent = message;
  editorHost.setAttribute("aria-busy", String(message != null));
}

function updateConnectionSummary(): void {
  const embedding =
    settings.embedProvider === "browser"
      ? "browser MiniLM"
      : settings.embedProvider === "http"
        ? settings.embedModel || "HTTP embeddings"
        : "no embedder";
  connectionSummary.textContent = `${settings.qdrantUrl} · ${embedding}`;
}

function releaseRetiredClients(): void {
  if (activeExecutions !== 0) return;
  for (const retired of retiredClients) retired.free();
  retiredClients.clear();
}

function configureClient(): void {
  const previous = client;
  const next = new Client(settings.qdrantUrl, settings.qdrantKey || null);

  if (settings.embedProvider === "browser") {
    next.setEmbedder(
      createBrowserEmbedder((message) => {
        embedStatus.textContent = message;
        setRuntime(message);
      }),
    );
    embedStatus.textContent =
      "Browser model loads only when execution needs text embeddings.";
  } else if (settings.embedProvider === "http") {
    if (!settings.embedUrl || !settings.embedModel || settings.embedDim < 1) {
      next.free();
      throw new Error("HTTP embeddings need an endpoint, model, and dimension.");
    }
    next.setHttpEmbedder(
      settings.embedUrl,
      settings.embedModel,
      settings.embedDim,
      settings.embedKey || null,
    );
    embedStatus.textContent = `Using ${settings.embedModel} through ${settings.embedUrl}.`;
  } else {
    embedStatus.textContent = "Text embedding is disabled; use explicit vectors.";
  }

  client = next;
  if (previous) {
    if (activeExecutions === 0) previous.free();
    else retiredClients.add(previous);
  }
  updateConnectionSummary();
}

function analyzeWithPolicy(source: string): PlaygroundAnalysis {
  const started = performance.now();
  const result = analyze(source);
  let effectiveAst = result.ast;
  let effectiveRoutes = result.routes;
  let policyError: string | null = null;

  if (result.valid && policy.enabled) {
    if (result.statements_count !== 1) {
      policyError =
        "Runtime policy injection accepts exactly one statement. Multi-statement scripts fail closed.";
    } else {
      let statement: Stmt | null = null;
      try {
        statement = new Stmt(source);
        statement.injectFilter(
          policy.field,
          policy.op,
          policyValue(policy),
        );
        if (policy.shardKey.trim()) statement.shardKey = policy.shardKey.trim();
        effectiveAst = [statement.toObject()];
        effectiveRoutes = [statement.compileRoute()];
      } catch (error) {
        policyError = formatError(error);
      } finally {
        statement?.free();
      }
    }
  }

  state.metrics = {
    analyzedAt: new Date().toISOString(),
    parseMs: performance.now() - started,
    executeMs: state.metrics?.executeMs ?? null,
    statements: result.statements_count,
    policyApplied: policy.enabled && !policyError && result.valid,
    embedProvider: settings.embedProvider,
  };

  return { source, result, effectiveAst, effectiveRoutes, policyError };
}

function currentDiagnostic(): Diagnostic[] {
  const analysis = state.analysis;
  if (!analysis) return [];
  const { source, result, policyError } = analysis;
  if (policyError) {
    return [
      {
        from: 0,
        to: Math.max(0, source.length),
        severity: "error",
        message: policyError,
      },
    ];
  }
  if (result.valid || !result.error) return [];
  const from =
    result.error.start == null
      ? 0
      : byteOffsetToPosition(source, result.error.start);
  const rawTo =
    result.error.end == null
      ? from + 1
      : byteOffsetToPosition(source, result.error.end);
  return [
    {
      from: Math.min(from, source.length),
      to: Math.min(Math.max(from + 1, rawTo), source.length),
      severity: "error",
      message: `${result.error.code}: ${result.error.message}`,
    },
  ];
}

function selectedRoute(): CompiledRoute | null {
  return (
    state.analysis?.effectiveRoutes[state.selectedStatement] ??
    state.analysis?.result.route ??
    null
  );
}

function renderStatementSelect(): void {
  const analysis = state.analysis;
  const count = Math.max(
    analysis?.result.statements_count ?? 0,
    analysis?.effectiveRoutes.length ?? 0,
  );
  statementSelect.replaceChildren();
  for (let index = 0; index < count; index += 1) {
    const option = document.createElement("option");
    option.value = String(index);
    option.textContent = `Statement ${index + 1}`;
    statementSelect.append(option);
  }
  state.selectedStatement = Math.min(
    state.selectedStatement,
    Math.max(0, count - 1),
  );
  statementSelect.value = String(state.selectedStatement);
  statementSelect.disabled = count <= 1;
}

function renderPlan(): void {
  const route = selectedRoute();
  planEmpty.hidden = Boolean(route);
  planCard.hidden = !route;
  if (!route) return;
  routeMethod.textContent = route.method;
  routePath.textContent = route.path;
  routeType.textContent = route.stmt_type;
  routePolicy.textContent = policy.enabled ? "Trusted predicate injected" : "Source only";
}

function selectStatement(index: number): void {
  state.selectedStatement = index;
  statementSelect.value = String(index);
  renderPlan();
  renderOutputs();
  renderRoutes();
}

function renderRoutes(): void {
  const routes = state.analysis?.effectiveRoutes ?? [];
  routesSection.hidden = routes.length === 0;
  routesCount.textContent = `${routes.length} ${routes.length === 1 ? "statement" : "statements"}`;
  routesList.replaceChildren();

  routes.forEach((route, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "route-row";
    item.setAttribute("role", "option");
    item.setAttribute(
      "aria-selected",
      String(index === state.selectedStatement),
    );
    item.setAttribute("aria-current", String(index === state.selectedStatement));
    item.dataset.routeIndex = String(index);

    const indexLabel = document.createElement("span");
    indexLabel.className = "route-row__index";
    indexLabel.textContent = String(index + 1);
    const typeLabel = document.createElement("span");
    typeLabel.className = "route-row__type";
    typeLabel.textContent = route.stmt_type;
    const methodLabel = document.createElement("span");
    methodLabel.className = "route-row__method";
    methodLabel.textContent = route.method;
    const pathLabel = document.createElement("span");
    pathLabel.className = "route-row__path";
    pathLabel.textContent = route.path;

    item.append(indexLabel, typeLabel, methodLabel, pathLabel);
    item.addEventListener("click", () => selectStatement(index));
    routesList.append(item);
  });
}

function renderOutputs(): void {
  const analysis = state.analysis;
  const route = selectedRoute();
  const ast = analysis?.effectiveAst?.[state.selectedStatement] ?? null;
  const responseOutput =
    state.executionError != null
      ? { ok: false, error: state.executionError }
      : state.response;

  const values: Record<Exclude<InspectorTab, "plan">, unknown> = {
    wire: route
      ? {
          method: route.method,
          path: route.path,
          payload: route.payload,
        }
      : "No compiled route.",
    ast: ast ?? "No AST output.",
    tokens:
      analysis?.result.tokens.map((token) => ({
        kind: token.kind,
        text: token.text,
        span: [token.pos, token.end],
      })) ?? "No tokens.",
    explain:
      analysis?.result.explain ??
      (analysis?.policyError
        ? analysis.policyError
        : "No explanation is available."),
    response: responseOutput ?? "Run a valid query to see the Qdrant response.",
    metrics:
      state.metrics ?? "Analysis metrics are not available.",
  };

  for (const [name, value] of Object.entries(values)) {
    required<HTMLElement>(`[data-output="${name}"]`).textContent =
      typeof value === "string" ? value : pretty(value);
  }
}

function renderValidation(): void {
  const analysis = state.analysis;
  const valid =
    analysis?.result.valid === true && analysis.policyError == null;
  validationBadge.classList.remove("is-loading", "is-valid", "is-invalid");
  validationBadge.classList.add(valid ? "is-valid" : "is-invalid");

  if (!analysis) {
    validationBadge.textContent = "WASM unavailable";
    analysisSummary.textContent = "The current qql-wasm package could not load.";
  } else if (analysis.policyError) {
    validationBadge.innerHTML = '<span class="status-dot"></span>Policy blocked';
    analysisSummary.textContent = analysis.policyError;
  } else if (analysis.result.valid) {
    validationBadge.innerHTML = '<span class="status-dot"></span>Valid QQL';
    analysisSummary.textContent = `${analysis.result.statements_count} ${
      analysis.result.statements_count === 1 ? "statement" : "statements"
    } · ${analysis.result.tokens.length} tokens · ${state.metrics?.parseMs.toFixed(2)} ms`;
  } else {
    validationBadge.innerHTML = '<span class="status-dot"></span>Invalid QQL';
    analysisSummary.textContent =
      analysis.result.error?.message ?? "The parser rejected this input.";
  }

  runButton.disabled = !valid;
  exportButton.disabled = !valid;
}

function renderInspector(): void {
  renderStatementSelect();
  renderPlan();
  renderOutputs();
  renderRoutes();
  renderValidation();
}

function runAnalysis(source: string): void {
  try {
    state.analysis = analyzeWithPolicy(source);
  } catch (error) {
    const message = formatError(error);
    state.analysis = {
      source,
      result: {
        valid: false,
        statements_count: 0,
        tokens: [],
        ast: null,
        route: null,
        routes: [],
        explain: null,
        error: {
          code: "QQL-WASM",
          message,
          start: null,
          end: null,
        },
      },
      effectiveAst: null,
      effectiveRoutes: [],
      policyError: null,
    };
  }
  state.selectedStatement = 0;
  renderInspector();
  forceLinting(editor);
}

function queueAnalysis(source: string): void {
  if (analysisTimer != null) window.clearTimeout(analysisTimer);
  analysisTimer = window.setTimeout(() => {
    analysisTimer = null;
    runAnalysis(source);
  }, ANALYSIS_DELAY_MS);
}

function switchInspectorTab(tab: InspectorTab, focus = false): void {
  state.inspectorTab = tab;
  saveSession(INSPECTOR_TAB_KEY, tab);
  for (const button of all<HTMLButtonElement>("[data-inspector-tab]")) {
    const active = button.dataset.inspectorTab === tab;
    button.setAttribute("aria-selected", String(active));
    button.tabIndex = active ? 0 : -1;
    if (active && focus) button.focus();
  }
  for (const panel of all<HTMLElement>("[data-inspector-panel]")) {
    panel.hidden = panel.dataset.inspectorPanel !== tab;
  }
}

function setupTabs(): void {
  const tabs = all<HTMLButtonElement>("[data-inspector-tab]");
  tabs.forEach((button, index) => {
    button.addEventListener("click", () => {
      switchInspectorTab(button.dataset.inspectorTab as InspectorTab);
    });
    button.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      const direction = event.key === "ArrowRight" ? 1 : -1;
      const next = (index + direction + tabs.length) % tabs.length;
      switchInspectorTab(
        tabs[next].dataset.inspectorTab as InspectorTab,
        true,
      );
    });
  });
}

function openDialog(id: string): void {
  const dialog = required<HTMLDialogElement>(id);
  dialog.showModal();
  if (id === "#preset-dialog") {
    window.setTimeout(() => required<HTMLInputElement>("[data-preset-search]").focus(), 0);
  }
}

function setupDialogs(): void {
  for (const dialog of all<HTMLDialogElement>("dialog[data-app-dialog]")) {
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) dialog.close();
    });
    for (const close of all<HTMLButtonElement>(
      `#${dialog.id} [data-close-dialog]`,
    )) {
      close.addEventListener("click", () => dialog.close());
    }
  }
  required("[data-open-presets]").addEventListener("click", () =>
    openDialog("#preset-dialog"),
  );
  required("[data-open-settings]").addEventListener("click", () =>
    openDialog("#settings-dialog"),
  );
  required("[data-open-policy]").addEventListener("click", () =>
    openDialog("#policy-dialog"),
  );
  exportButton.addEventListener("click", () => {
    renderExport();
    openDialog("#export-dialog");
  });
}

function setupPresets(): void {
  const search = required<HTMLInputElement>("[data-preset-search]");
  const results = required<HTMLElement>("[data-preset-results]");
  const cards = all<HTMLButtonElement>("[data-preset]");
  const categories = all<HTMLButtonElement>("[data-preset-category]");
  let category = "all";

  const filter = () => {
    const query = search.value.trim().toLowerCase();
    for (const card of cards) {
      const categoryMatches =
        category === "all" || card.dataset.presetCategoryValue === category;
      const text = [
        card.dataset.presetLabel,
        card.dataset.presetDescription,
        card.dataset.presetId,
      ]
        .join(" ")
        .toLowerCase();
      card.hidden = !categoryMatches || !text.includes(query);
    }
    const shown = cards.filter((card) => !card.hidden).length;
    results.textContent = `${shown} of ${cards.length} fixture examples shown`;
  };

  search.addEventListener("input", filter);
  for (const button of categories) {
    button.addEventListener("click", () => {
      category = button.dataset.presetCategory ?? "all";
      categories.forEach((item) => item.setAttribute("aria-pressed", String(item === button)));
      filter();
    });
  }
  for (const card of cards) {
    card.addEventListener("click", () => {
      const query = card.dataset.presetQuery ?? "";
      editor.dispatch({
        changes: { from: 0, to: editor.state.doc.length, insert: query },
      });
      activeFixture.textContent = card.dataset.presetLabel ?? "Custom query";
      required<HTMLDialogElement>("#preset-dialog").close();
      editor.focus();
    });
    card.addEventListener("keydown", (event) => {
      const visible = cards.filter((item) => !item.hidden);
      const index = visible.indexOf(card);
      if (index === -1) return;
      let next: number | null = null;
      if (event.key === "Home") next = 0;
      if (event.key === "End") next = visible.length - 1;
      if (event.key === "ArrowRight" || event.key === "ArrowDown") next = index + 1;
      if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = index - 1;
      if (next == null) return;
      event.preventDefault();
      visible[(next + visible.length) % visible.length]?.focus();
    });
  }
}

function setupShare(): void {
  shareButton.addEventListener("click", async () => {
    const url = new URL(window.location.href);
    url.search = "";
    url.searchParams.set("q", editor.state.doc.toString());
    const ref = docsBacklink.hidden ? null : docsBacklink.getAttribute("href");
    if (ref?.startsWith("/docs/")) url.searchParams.set("ref", ref);
    try {
      await navigator.clipboard.writeText(url.toString());
      toast("Share link copied.");
    } catch {
      toast("Clipboard access was denied. Copy the page URL instead.", "error");
    }
  });
}

function setupDocsBacklink(ref: string | null): void {
  if (!ref || !/^\/docs(?:\/|$)/.test(ref)) return;
  docsBacklink.href = ref;
  docsBacklink.hidden = false;
}

function writeSettingsForm(): void {
  const form = required<HTMLFormElement>("[data-settings-form]");
  const field = <T extends HTMLInputElement | HTMLSelectElement>(name: string) =>
    required<T>(`[data-settings-form] [name="${name}"]`);
  field<HTMLInputElement>("qdrantUrl").value = settings.qdrantUrl;
  field<HTMLInputElement>("qdrantKey").value = settings.qdrantKey;
  field<HTMLSelectElement>("embedProvider").value = settings.embedProvider;
  field<HTMLInputElement>("embedUrl").value = settings.embedUrl;
  field<HTMLInputElement>("embedModel").value = settings.embedModel;
  field<HTMLInputElement>("embedDim").value = String(settings.embedDim);
  field<HTMLInputElement>("embedKey").value = settings.embedKey;

  const toggleHttpFields = () => {
    required<HTMLFieldSetElement>("[data-http-embed-fields]").hidden =
      field<HTMLSelectElement>("embedProvider").value !== "http";
  };
  field<HTMLSelectElement>("embedProvider").addEventListener(
    "change",
    toggleHttpFields,
  );
  toggleHttpFields();

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    settings = {
      qdrantUrl: field<HTMLInputElement>("qdrantUrl").value.replace(/\/+$/, ""),
      qdrantKey: field<HTMLInputElement>("qdrantKey").value,
      embedProvider: field<HTMLSelectElement>("embedProvider")
        .value as PlaygroundSettings["embedProvider"],
      embedUrl: field<HTMLInputElement>("embedUrl").value,
      embedModel: field<HTMLInputElement>("embedModel").value,
      embedDim: Number(field<HTMLInputElement>("embedDim").value),
      embedKey: field<HTMLInputElement>("embedKey").value,
    };
    try {
      configureClient();
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
      required<HTMLDialogElement>("#settings-dialog").close();
      runAnalysis(editor.state.doc.toString());
      toast("Connection settings saved.");
    } catch (error) {
      toast(formatError(error), "error");
    }
  });
}

function writePolicyForm(): void {
  const form = required<HTMLFormElement>("[data-policy-form]");
  const field = <T extends HTMLInputElement | HTMLSelectElement>(name: string) =>
    required<T>(`[data-policy-form] [name="${name}"]`);
  field<HTMLInputElement>("enabled").checked = policy.enabled;
  field<HTMLInputElement>("field").value = policy.field;
  field<HTMLSelectElement>("op").value = policy.op;
  field<HTMLInputElement>("value").value = policy.value;
  field<HTMLSelectElement>("valueType").value = policy.valueType;
  field<HTMLInputElement>("shardKey").value = policy.shardKey;
  policyDot.classList.toggle("is-active", policy.enabled);

  const preview = required<HTMLOutputElement>("[data-policy-preview]");
  const updatePreview = () => {
    const enabled = field<HTMLInputElement>("enabled").checked;
    const fieldName = field<HTMLInputElement>("field").value.trim() || "field";
    const operator = field<HTMLSelectElement>("op").value;
    const value = field<HTMLInputElement>("value").value || "value";
    const shard = field<HTMLInputElement>("shardKey").value.trim();
    preview.textContent = enabled
      ? `Trusted filter: ${fieldName} ${operator} ${value}${shard ? ` · route with shard ${shard}` : ""}`
      : "The source stays unchanged. Enable the guardrail to inject a trusted predicate.";
  };
  updatePreview();

  for (const name of ["enabled", "field", "op", "value", "shardKey"] as const) {
    field<HTMLInputElement | HTMLSelectElement>(name).addEventListener("input", updatePreview);
    field<HTMLInputElement | HTMLSelectElement>(name).addEventListener("change", updatePreview);
  }

  for (const recipe of all<HTMLButtonElement>("[data-policy-recipe]")) {
    recipe.addEventListener("click", () => {
      field<HTMLInputElement>("enabled").checked = true;
      field<HTMLInputElement>("field").value = recipe.dataset.field ?? "";
      field<HTMLInputElement>("value").value = recipe.dataset.value ?? "";
      field<HTMLSelectElement>("valueType").value =
        recipe.dataset.valueType ?? "string";
      field<HTMLInputElement>("shardKey").value = recipe.dataset.shard ?? "";
      updatePreview();
    });
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    policy = {
      enabled: field<HTMLInputElement>("enabled").checked,
      field: field<HTMLInputElement>("field").value.trim(),
      op: field<HTMLSelectElement>("op").value,
      value: field<HTMLInputElement>("value").value,
      valueType: field<HTMLSelectElement>("valueType")
        .value as PolicyValueType,
      shardKey: field<HTMLInputElement>("shardKey").value,
    };
    localStorage.setItem(POLICY_KEY, JSON.stringify(policy));
    policyDot.classList.toggle("is-active", policy.enabled);
    required<HTMLDialogElement>("#policy-dialog").close();
    runAnalysis(editor.state.doc.toString());
    toast(policy.enabled ? "Runtime policy applied." : "Runtime policy disabled.");
  });
}

function renderExport(): void {
  const source = editor.state.doc.toString();
  required<HTMLElement>("[data-export-output]").textContent = exportCode(
    state.exportLanguage,
    source,
    settings,
    selectedRoute(),
    state.analysis?.result.statements_count ?? 1,
  );
}

function setupExporter(): void {
  const tabs = all<HTMLButtonElement>("[data-export-tab]");
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      state.exportLanguage = tab.dataset.exportTab as ExportLanguage;
      tabs.forEach((item) =>
        item.setAttribute("aria-pressed", String(item === tab)),
      );
      renderExport();
    });
  }
  required("[data-copy-export]").addEventListener("click", async () => {
    const code = required<HTMLElement>("[data-export-output]").textContent ?? "";
    try {
      await navigator.clipboard.writeText(code);
      toast("SDK code copied.");
    } catch {
      toast("Clipboard access was denied.", "error");
    }
  });
}

async function executeQuery(): Promise<void> {
  if (!client || !state.analysis?.result.valid || state.analysis.policyError) {
    return;
  }
  runButton.disabled = true;
  runButton.classList.add("is-running");
  runButton.textContent = "Running…";
  state.executionError = null;
  state.response = null;
  renderOutputs();
  setRuntime("Executing against Qdrant…");

  const executionClient = client;
  const source = editor.state.doc.toString();
  const started = performance.now();
  let statement: Stmt | null = null;
  activeExecutions += 1;

  try {
    if (policy.enabled) {
      statement = new Stmt(source);
      statement.injectFilter(
        policy.field,
        policy.op,
        policyValue(policy),
      );
      if (policy.shardKey.trim()) statement.shardKey = policy.shardKey.trim();
      state.response = await executionClient.executeStmt(statement);
    } else {
      state.response = await executionClient.execute(source);
    }
    if (state.metrics) state.metrics.executeMs = performance.now() - started;
    setRuntime(
      state.response.ok
        ? `Execution complete · ${state.response.succeeded} succeeded`
        : `Execution complete · ${state.response.failed} failed`,
    );
    switchInspectorTab("response");
    toast(
      state.response.ok ? "Query executed." : "Qdrant returned a failure.",
      state.response.ok ? "success" : "error",
    );
  } catch (error) {
    state.executionError = formatError(error);
    if (state.metrics) state.metrics.executeMs = performance.now() - started;
    setRuntime("Execution failed.");
    switchInspectorTab("response");
    toast(state.executionError, "error");
  } finally {
    statement?.free();
    activeExecutions -= 1;
    releaseRetiredClients();
    runButton.classList.remove("is-running");
    runButton.innerHTML =
      '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m8 5 11 7-11 7V5Z"></path></svg>Run<kbd class="hidden rounded border border-white/25 px-1 font-mono text-[0.55rem] opacity-80 sm:inline">⌘↵</kbd>';
    renderValidation();
    renderOutputs();
  }
}

const pageParams = new URLSearchParams(window.location.search);
const urlQuery = pageParams.get("q");
const initialQuery =
  urlQuery || loadSession(WORKSPACE_KEY) || workspace.dataset.defaultQuery || "";

const editor = new EditorView({
  parent: editorHost,
  state: EditorState.create({
    doc: initialQuery,
    extensions: [
      basicSetup,
      qqlLanguage,
      qqlHighlighting,
      qqlCompletion,
      lintGutter(),
      linter(currentDiagnostic),
      keymap.of([
        indentWithTab,
        {
          key: "Mod-Enter",
          run: () => {
            void executeQuery();
            return true;
          },
        },
        {
          key: "Mod-k",
          run: () => {
            openDialog("#preset-dialog");
            return true;
          },
        },
      ]),
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        activeFixture.textContent = "Custom query";
        saveSession(WORKSPACE_KEY, update.state.doc.toString());
        queueAnalysis(update.state.doc.toString());
      }),
    ],
  }),
});

async function start(): Promise<void> {
  setupTabs();
  setupDialogs();
  setupPresets();
  setupShare();
  setupDocsBacklink(pageParams.get("ref"));
  writeSettingsForm();
  writePolicyForm();
  setupExporter();
  const savedTab = loadSession(INSPECTOR_TAB_KEY);
  if (["plan", "wire", "ast", "tokens", "explain", "response", "metrics"].includes(savedTab ?? "")) {
    switchInspectorTab(savedTab as InspectorTab);
  }
  statementSelect.addEventListener("change", () => {
    state.selectedStatement = Number(statementSelect.value);
    renderPlan();
    renderOutputs();
    renderRoutes();
  });
  runButton.addEventListener("click", () => void executeQuery());

  try {
    await initQql();
    configureClient();
    setRuntime("Current qql-rs WASM ready");
    runAnalysis(editor.state.doc.toString());
    setEditorLoading(null);
  } catch (error) {
    const message = formatError(error);
    setRuntime(`WASM failed: ${message}`);
    validationBadge.classList.remove("is-loading");
    validationBadge.classList.add("is-invalid");
    validationBadge.textContent = "WASM unavailable";
    analysisSummary.textContent = message;
    setEditorLoading("QQL WebAssembly could not load. Reload the page to try again.");
    toast(message, "error");
  }
}

window.addEventListener("beforeunload", () => {
  client?.free();
  for (const retired of retiredClients) retired.free();
  retiredClients.clear();
});

void start();
