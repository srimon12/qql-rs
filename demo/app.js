import init, { analyze, Client } from './pkg/qql_wasm.js?v=4';

// ── Canonical QQL Presets (Valid Syntax with Trailing Semicolons) ────────────

const PRESETS = {
  hybrid: `-- 🚀 Hybrid Dense+Sparse RRF — RTX missile defense contracts
-- Embeds text → queries both dense & sparse vectors → fuses with RRF
QUERY HYBRID TEXT 'Raytheon missile defense contracts programs'
  DENSE dense
  SPARSE sparse
  FUSION RRF
  FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'rtx'
  WITH PAYLOAD true
  LIMIT 5;`,

  cte: `-- ⚡ CTE Prefetch DAG + Fusion + Score Threshold — Honeywell
-- Stage 1: dense & sparse CTE pre-fetches with independent filters
-- Stage 2: RRF fusion with per-stream score cutoffs
WITH
  dense_candidates AS (
    QUERY TEXT 'supply chain disruption risk shortages'
    FROM sec10k USING dense
    WHERE fiscal_year >= 2024 LIMIT 100
  ),
  sparse_candidates AS (
    QUERY TEXT 'supply chain disruption risk shortages'
    FROM sec10k USING sparse
    WHERE fiscal_year >= 2024 LIMIT 100
  )
QUERY FUSION RRF FROM sec10k
  PREFETCH (
    dense_candidates SCORE THRESHOLD 0.4,
    sparse_candidates SCORE THRESHOLD 0.2
  )
  SHARD 'honeywell'
  WITH PAYLOAD true
  LIMIT 5;`,

  formula: `-- 🧮 Formula Score Boosting — RTX financial results boosted 2x
-- Stage 1: dense CTE pre-fetch finds financial chunks
-- Stage 2: FORMULA multiplies every score by 2.0 with DEFAULTS fallback
WITH
  candidates AS (
    QUERY TEXT 'financial results revenue earnings growth margins'
    FROM sec10k USING dense
    WHERE fiscal_year >= 2024 LIMIT 30
  )
QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0)
  FROM sec10k
  PREFETCH (candidates)
  SHARD 'rtx'
  WITH PAYLOAD true
  LIMIT 5;`,

  grouped: `-- 📊 Grouped Aggregation by Fiscal Year — RTX financials
-- Hybrid RRF query with GROUP BY — 3 top hits per fiscal year
QUERY HYBRID TEXT 'financial results revenue earnings'
  DENSE dense SPARSE sparse
  FUSION RRF
  FROM sec10k
  WHERE has_figures = true
  SHARD 'rtx'
  GROUP BY fiscal_year SIZE 3
  LIMIT 20;`,

  mmr: `-- 🎯 MMR Diversified Results — 3M manufacturing innovation
-- Maximal Marginal Relevance: DIVERSITY 0.5 avoids near-duplicates
-- CANDIDATES 100 fetches a larger pool before diversity pruning
QUERY MMR TEXT 'manufacturing operations innovation products'
  DIVERSITY 0.5 CANDIDATES 100
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
  SHARD '3m'
  PARAMS (hnsw_ef = 256)
  WITH PAYLOAD true
  LIMIT 5;`,

  ddl: `-- 📜 SCROLL Pagination — GE filings with Cursor
-- Scroll over GE's 2024+ chunks; use AFTER <point_id> to paginate
SCROLL FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'ge'
  LIMIT 5;

-- Count total chunks with financial figures across all companies
COUNT FROM sec10k
  WHERE has_figures = true AND fiscal_year >= 2024;`,

  discover: `-- 🔄 DBSF Alternative Fusion — Honeywell supply chain
-- Distribution-Based Score Fusion instead of RRF
QUERY HYBRID TEXT 'supply chain disruption risk shortages'
  DENSE dense SPARSE sparse
  FUSION DBSF
  FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'honeywell'
  WITH PAYLOAD true
  LIMIT 5;`,

  mutation: `-- 🗑️ Upsert + Cleanup — demo point lifecycle
UPSERT INTO sec10k VALUES
  {id: 9999999, text: 'QQL: vector query language for Qdrant — WASM-powered',
   tenant_id: 'rtx', company: 'demo', fiscal_year: 2026}
USING DENSE MODEL 'text-embedding-all-minilm-l6-v2-embedding'
SHARD 'rtx';

DELETE FROM sec10k WHERE id = 9999999 AND tenant_id = 'rtx';`
};

// Token kind mapping to CSS classes
const KIND_CLASS_MAP = {
  String: 'hl-str',
  Number: 'hl-num',
  Comment: 'hl-cmt',
  Eq: 'hl-op', Lt: 'hl-op', Gt: 'hl-op', Colon: 'hl-op',
  Comma: 'hl-op', Lbrace: 'hl-op', Rbrace: 'hl-op',
  Lparen: 'hl-op', Rparen: 'hl-op',
  AND: 'hl-op', OR: 'hl-op', NOT: 'hl-op', IN: 'hl-op', BETWEEN: 'hl-op',
  IS: 'hl-op', NULL: 'hl-op', EMPTY: 'hl-op', MATCH: 'hl-op', ANY: 'hl-op', PHRASE: 'hl-op'
};

const KEYWORD_KINDS = new Set([
  'UPSERT', 'INTO', 'VALUES', 'QUERY', 'FROM', 'SELECT', 'SCROLL', 'CREATE',
  'COLLECTION', 'COLLECTIONS', 'ALTER', 'DROP', 'SHOW', 'DELETE', 'UPDATE',
  'SET', 'VECTOR', 'PAYLOAD', 'WHERE', 'INDEX', 'WITH', 'USING', 'MODEL',
  'HYBRID', 'SPARSE', 'DENSE', 'RERANK', 'HNSW', 'QUANTIZATION', 'OPTIMIZERS',
  'PARAMS', 'PREFETCH', 'FUSION', 'RRF', 'DBSF', 'AS', 'LIMIT', 'OFFSET',
  'GROUP', 'BY', 'SIZE', 'EXACT', 'ACORN', 'BOOST', 'DEFAULTS', 'CASE',
  'WHEN', 'THEN', 'ELSE', 'END', 'ON', 'FOR', 'TYPE', 'ORDER', 'ASC',
  'DESC', 'RECOMMEND', 'DISCOVER', 'CONTEXT', 'PAIRS', 'TARGET', 'SAMPLE',
  'STRATEGY', 'SCORE', 'THRESHOLD', 'LOOKUP', 'TRUE', 'FALSE', 'POINTS',
  'HAS_VECTOR', 'VALUES_COUNT', 'GEO_BBOX', 'GEO_RADIUS', 'NESTED',
  'COSINE', 'DOT', 'EUCLID', 'MANHATTAN', 'MULTIVECTOR', 'EMBED', 'AFTER'
]);

function getCssClassForToken(kind) {
  if (KEYWORD_KINDS.has(kind)) return 'hl-kw';
  return KIND_CLASS_MAP[kind] || 'hl-id';
}

// ── State ─────────────────────────────────────────────────────────────────────

let wasmReady = false;
let client = null;
let currentAnalysis = null;
let debounceTimer = null;

// ── DOM Elements ──────────────────────────────────────────────────────────────

const queryInput = document.getElementById('query-input');
const highlightLayer = document.getElementById('editor-highlight-layer');
const lineNumbers = document.getElementById('line-numbers');
const statusBadge = document.getElementById('status-badge');
const statusIcon = document.getElementById('status-icon');
const statusText = document.getElementById('status-text');
const latencyTag = document.getElementById('latency-tag');
const btnExecute = document.getElementById('btn-execute');
const presetSelect = document.getElementById('preset-select');
const errorFooter = document.getElementById('editor-error-footer');
const errorMessageText = document.getElementById('error-message-text');

// Modal Elements
const settingsModal = document.getElementById('settings-modal');
const btnOpenSettings = document.getElementById('btn-open-settings');
const btnCloseSettings = document.getElementById('btn-close-settings');
const btnSaveSettings = document.getElementById('btn-save-settings');

const cfgQdrantUrl = document.getElementById('cfg-qdrant-url');
const cfgQdrantKey = document.getElementById('cfg-qdrant-key');
const cfgEmbedProvider = document.getElementById('cfg-embed-provider');
const cfgEmbedUrl = document.getElementById('cfg-embed-url');
const cfgEmbedModel = document.getElementById('cfg-embed-model');
const cfgEmbedDim = document.getElementById('cfg-embed-dim');
const cfgEmbedKey = document.getElementById('cfg-embed-key');
const cfgStatusMsg = document.getElementById('cfg-status-msg');

// Inspector Elements
const routeCardContainer = document.getElementById('route-card-container');
const routeMethod = document.getElementById('route-method');
const routePath = document.getElementById('route-path');
const routeSummary = document.getElementById('route-summary');
const planExplanationBox = document.getElementById('plan-explanation-box');

const codeWireJson = document.getElementById('code-wire-json');
const codeAstJson = document.getElementById('code-ast-json');
const tokensTableBody = document.getElementById('tokens-table-body');
const explainBox = document.getElementById('explain-box');
const codeResponseJson = document.getElementById('code-response-json');

// ── Configure Client Instance ─────────────────────────────────────────────────

function configureClient() {
  const qdrantUrl = cfgQdrantUrl.value.trim() || "http://localhost:6333";
  const qdrantKey = cfgQdrantKey.value.trim() || undefined;

  client = new Client(qdrantUrl, qdrantKey);

  const provider = cfgEmbedProvider.value;
  if (provider === 'openai' || provider === 'remote') {
    const embedUrl = cfgEmbedUrl.value.trim() || "http://localhost:11434/v1/embeddings";
    const embedModel = cfgEmbedModel.value.trim() || "all-minilm:l6-v2";
    const embedDim = parseInt(cfgEmbedDim.value.trim()) || 384;
    const embedKey = cfgEmbedKey.value.trim() || "";

    client.setHttpEmbedder(embedUrl, embedModel, embedDim, embedKey || null);
    cfgStatusMsg.textContent = `✓ Configured: Qdrant (${qdrantUrl}) | Embedder (${embedModel} @ ${embedUrl})`;
    cfgStatusMsg.style.color = "var(--accent-emerald)";
  } else {
    cfgStatusMsg.textContent = `✓ Configured: Qdrant (${qdrantUrl}) | No Embedder`;
    cfgStatusMsg.style.color = "var(--accent-emerald)";
  }
}

// ── Unified Analysis & Live Highlight ─────────────────────────────────────────

function performAnalysis() {
  if (!wasmReady) return;

  const text = queryInput.value;
  updateLineNumbers(text);

  const t0 = performance.now();
  const res = JSON.parse(analyze(text));
  const t1 = performance.now();

  currentAnalysis = res;
  latencyTag.textContent = `⚡ ${(t1 - t0).toFixed(2)} ms`;

  // Render Live Highlight
  renderHighlight(text, res.tokens, res.error);

  // Update Status & Execute button state
  if (res.valid) {
    statusBadge.className = 'status-badge valid';
    statusIcon.textContent = '✓';
    statusText.textContent = res.statements_count > 1
      ? `${res.statements_count} Statements`
      : 'Valid';
    errorFooter.style.display = 'none';
    btnExecute.disabled = false;
  } else {
    statusBadge.className = 'status-badge error';
    statusIcon.textContent = '✗';
    statusText.textContent = res.error?.code || 'Error';
    errorFooter.style.display = 'flex';
    errorMessageText.textContent = res.error?.message
      ? `${res.error.code}: ${res.error.message}`
      : 'Parse error';
    btnExecute.disabled = true;
  }

  // Update Inspector Views
  updateInspectorViews(res);
}

function renderHighlight(sourceText, tokens, error) {
  if (!tokens || tokens.length === 0) {
    highlightLayer.innerHTML = escapeHtml(sourceText);
    return;
  }

  let html = '';
  let lastPos = 0;
  const errStart = error?.start;
  const errEnd = error?.end;

  for (const t of tokens) {
    if (t.pos > lastPos) {
      const skipped = sourceText.slice(lastPos, t.pos);
      html += formatSegment(skipped, lastPos, errStart, errEnd);
    }

    const cssCls = getCssClassForToken(t.kind);
    const tokenText = sourceText.slice(t.pos, t.end);
    const formattedToken = escapeHtml(tokenText);

    if (errStart !== undefined && errEnd !== undefined && t.pos >= errStart && t.end <= errEnd) {
      html += `<span class="${cssCls} hl-err">${formattedToken}</span>`;
    } else {
      html += `<span class="${cssCls}">${formattedToken}</span>`;
    }

    lastPos = t.end;
  }

  if (lastPos < sourceText.length) {
    const trailing = sourceText.slice(lastPos);
    html += formatSegment(trailing, lastPos, errStart, errEnd);
  }

  if (sourceText.endsWith('\n')) {
    html += '<br>';
  }

  highlightLayer.innerHTML = html;
}

function formatSegment(str, offset, errStart, errEnd) {
  if (errStart !== undefined && errEnd !== undefined && offset < errEnd && (offset + str.length) > errStart) {
    return `<span class="hl-err">${escapeHtml(str)}</span>`;
  }
  return escapeHtml(str);
}

function updateLineNumbers(text) {
  const count = text.split('\n').length;
  let nums = '';
  for (let i = 1; i <= count; i++) {
    nums += i + '\n';
  }
  lineNumbers.textContent = nums;
}

function escapeHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

// ── Sync Scroll between Textarea & Highlight Layer ─────────────────────────────

queryInput.addEventListener('scroll', () => {
  highlightLayer.scrollTop = queryInput.scrollTop;
  highlightLayer.scrollLeft = queryInput.scrollLeft;
  lineNumbers.scrollTop = queryInput.scrollTop;
});

// ── Inspector Views Update ─────────────────────────────────────────────────────

function updateInspectorViews(analysis) {
  // 1. Visual Plan View
  if (analysis.route) {
    const method = (analysis.route.method || 'GET').toLowerCase();
    routeMethod.className = `http-method ${method}`;
    routeMethod.textContent = (analysis.route.method || 'GET').toUpperCase();
    routePath.textContent = analysis.route.path || '/';

    routeSummary.textContent = analysis.statements_count > 1
      ? `Script contains ${analysis.statements_count} statements. Showing first statement.`
      : `Compiled QQL statement routed to Qdrant REST handler.`;

    codeWireJson.textContent = JSON.stringify(analysis.route.payload, null, 2);
  } else {
    routeMethod.className = 'http-method get';
    routeMethod.textContent = 'NONE';
    routePath.textContent = '/';
    routeSummary.textContent = analysis.error ? analysis.error.message : 'No route generated.';
    codeWireJson.textContent = analysis.error ? JSON.stringify(analysis.error, null, 2) : '{}';
  }

  planExplanationBox.textContent = analysis.explain || analysis.error?.message || 'No plan explanation available.';
  explainBox.textContent = analysis.explain || analysis.error?.message || 'No explanation available.';

  // 2. AST View
  codeAstJson.textContent = analysis.ast
    ? JSON.stringify(analysis.ast, null, 2)
    : (analysis.error ? JSON.stringify(analysis.error, null, 2) : '{}');

  // 3. Tokens Table
  if (analysis.tokens && analysis.tokens.length > 0) {
    let rows = '';
    for (const t of analysis.tokens) {
      const cls = getCssClassForToken(t.kind);
      rows += `<tr>
        <td class="${cls}">${t.kind}</td>
        <td>${escapeHtml(t.text)}</td>
        <td>${t.pos}</td>
        <td>${t.end}</td>
        <td>${t.len}</td>
      </tr>`;
    }
    tokensTableBody.innerHTML = rows;
  } else {
    tokensTableBody.innerHTML = `<tr><td colspan="5" style="color:var(--text-dim); text-align:center">No tokens generated</td></tr>`;
  }
}

// ── Tab Switching ─────────────────────────────────────────────────────────────

document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.tab-pane').forEach(p => p.classList.remove('active'));

    btn.classList.add('active');
    const targetId = `tab-${btn.dataset.tab}`;
    document.getElementById(targetId).classList.add('active');
  });
});

// ── Live Input Handler ────────────────────────────────────────────────────────

queryInput.addEventListener('input', () => {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(performAnalysis, 50);
});

// Tab key indent support
queryInput.addEventListener('keydown', (e) => {
  if (e.key === 'Tab') {
    e.preventDefault();
    const start = queryInput.selectionStart;
    const end = queryInput.selectionEnd;
    queryInput.value = queryInput.value.substring(0, start) + '  ' + queryInput.value.substring(end);
    queryInput.selectionStart = queryInput.selectionEnd = start + 2;
    performAnalysis();
  }
});

// ── Preset Selection ──────────────────────────────────────────────────────────

presetSelect.addEventListener('change', () => {
  const val = presetSelect.value;
  if (PRESETS[val]) {
    queryInput.value = PRESETS[val];
    performAnalysis();
  }
});

document.getElementById('btn-clear').addEventListener('click', () => {
  queryInput.value = '';
  performAnalysis();
});

document.getElementById('btn-format').addEventListener('click', () => {
  performAnalysis();
});

// ── Modal Settings Control ────────────────────────────────────────────────────

btnOpenSettings.addEventListener('click', () => {
  settingsModal.classList.add('open');
});

btnCloseSettings.addEventListener('click', () => {
  settingsModal.classList.remove('open');
});

btnSaveSettings.addEventListener('click', () => {
  configureClient();
  settingsModal.classList.remove('open');
});

// Close modal when clicking on backdrop
settingsModal.addEventListener('click', (e) => {
  if (e.target === settingsModal) {
    settingsModal.classList.remove('open');
  }
});

// ── Execute Button Action ─────────────────────────────────────────────────────

btnExecute.addEventListener('click', async () => {
  const text = queryInput.value.trim();
  if (!text) return;

  // Switch to Response Tab
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
  document.querySelectorAll('.tab-pane').forEach(p => p.classList.remove('active'));
  document.querySelector('.tab-btn[data-tab="response"]').classList.add('active');
  document.getElementById('tab-response').classList.add('active');

  if (!client) {
    configureClient();
  }

  try {
    codeResponseJson.textContent = 'Executing via QQL WASM Client (Embedding -> Qdrant REST)...';
    const resJson = await client.execute(text);
    const res = JSON.parse(resJson);
    codeResponseJson.textContent = JSON.stringify(res, null, 2);
  } catch (e) {
    const route = currentAnalysis?.route;
    codeResponseJson.textContent = JSON.stringify({
      error: String(e),
      note: "If Qdrant or Ollama is not running on localhost, check Settings (⚙️ Settings & Embedder).",
      route: route || null
    }, null, 2);
  }
});

// ── Application Main Entry Point ──────────────────────────────────────────────

async function main() {
  await init();
  wasmReady = true;

  // Prefill default client settings
  configureClient();

  // Load initial preset
  queryInput.value = PRESETS.hybrid;
  performAnalysis();
}

main().catch(console.error);
