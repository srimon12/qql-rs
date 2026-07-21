import init, { tokenize, parse, parse_all, compile, explain, isValid, Client } from './pkg/qql_wasm.js?v=2';

// ── State ──
let wasmReady = false;
let client = null;
let currentQuery = "QUERY 'search' FROM docs LIMIT 10";
let timer = null;

// ── Token coloring ──
const KIND_CLASS = {
  UPSERT: 'kw', INTO: 'kw', VALUES: 'kw', QUERY: 'kw', FROM: 'kw',
  SELECT: 'kw', SCROLL: 'kw', CREATE: 'kw', COLLECTION: 'kw', COLLECTIONS: 'kw',
  ALTER: 'kw', DROP: 'kw', SHOW: 'kw', DELETE: 'kw', UPDATE: 'kw',
  SET: 'kw', VECTOR: 'kw', PAYLOAD: 'kw', WHERE: 'kw', INDEX: 'kw',
  WITH: 'kw', USING: 'kw', MODEL: 'kw', HYBRID: 'kw', SPARSE: 'kw', DENSE: 'kw',
  RERANK: 'kw', HNSW: 'kw', QUANTIZATION: 'kw', OPTIMIZERS: 'kw', PARAMS: 'kw',
  PREFETCH: 'kw', FUSION: 'kw', RRF: 'kw', DBSF: 'kw', AS: 'kw',
  LIMIT: 'kw', OFFSET: 'kw', GROUP: 'kw', BY: 'kw', GROUP_SIZE: 'kw',
  EXACT: 'kw', ACORN: 'kw', BOOST: 'kw', DEFAULTS: 'kw', CASE: 'kw',
  WHEN: 'kw', THEN: 'kw', ELSE: 'kw', END: 'kw', ON: 'kw', FOR: 'kw', TYPE: 'kw',
  ORDER: 'kw', ASC: 'kw', DESC: 'kw', RECOMMEND: 'kw', DISCOVER: 'kw',
  CONTEXT: 'kw', PAIRS: 'kw', TARGET: 'kw', SAMPLE: 'kw', STRATEGY: 'kw',
  SCORE: 'kw', THRESHOLD: 'kw', LOOKUP: 'kw', TRUE: 'kw', FALSE: 'kw',
  AND: 'op', OR: 'op', NOT: 'op', IN: 'op', BETWEEN: 'op',
  IS: 'op', NULL: 'op', EMPTY: 'op', MATCH: 'op', ANY: 'op', PHRASE: 'op',
  Lbrace: 'op', Rbrace: 'op', Lparen: 'op', Rparen: 'op', Comma: 'op',
  Eq: 'op', Lt: 'op', Gt: 'op', Colon: 'op',
  String: 'str', Number: 'num', Comment: 'cmt',
  HAS_VECTOR: 'kw', VALUES_COUNT: 'kw', GEO_BBOX: 'kw', GEO_RADIUS: 'kw',
  NESTED: 'kw', COSINE: 'kw', DOT: 'kw', EUCLID: 'kw', MANHATTAN: 'kw',
  MULTIVECTOR: 'kw', EMBED: 'kw', AFTER: 'kw',
};

function tokenClass(kind) {
  return KIND_CLASS[kind] || 'id';
}

async function debouncedRefresh() {
  if (timer) clearTimeout(timer);
  timer = setTimeout(refresh, 150);
}

// ── Core refresh ──
async function refresh() {
  if (!wasmReady) return;
  const q = document.getElementById('query-input').value.trim();
  currentQuery = q;
  if (!q) {
    setStatus('', '');
    showHighlight('');
    showTokens([]);
    showExplain('');
    showCompile(null);
    showResponse(null);
    return;
  }

  // Tokenize
  let tokens = [];
  try { tokens = tokenize(q); } catch (e) {}
  showHighlight(q, tokens);

  // Validate (multi-statement aware)
  let valid = false;
  let multi = false;
  try {
    const stmts = parse_all(q);
    valid = stmts.length > 0;
    multi = stmts.length > 1;
    showAST(multi ? stmts : (stmts[0] || null));
  } catch (e) {
    showAST(null, e);
  }
  setStatus(valid
    ? (multi ? `✓ ${parse_all(q).length} statements` : '✓ Valid')
    : '✗ Error', valid ? 'ok' : 'err');
  document.getElementById('btn-execute').disabled = !valid || !client;

  showTokens(tokens);

  // Explain
  try {
    showExplain(explain(q));
  } catch (e) {
    if (multi) {
      showExplain(`${new Set(parse_all(q).map(s => Object.keys(s).find(k => !k.startsWith('_') && typeof s[k] === 'object') || '?')).size} statement types\n(explain shows first statement only)`);
    } else {
      showExplain('Parse error:\n' + e);
    }
  }

  // Compile
  try {
    showCompile(compile(q));
  } catch (e) {
    if (multi) {
      showCompile(null, `Multi-statement script (${parse_all(q).length} statements). Select a single statement to compile.`);
    } else {
      showCompile(null, e);
    }
  }
}

// ── Display helpers ──
function setStatus(text, cls) {
  const el = document.getElementById('validation-status');
  el.textContent = text || '...';
  el.className = 'status ' + cls;
}

function showHighlight(source, tokens) {
  const el = document.getElementById('highlight-view');
  if (!tokens || !tokens.length) { el.innerHTML = ''; return; }
  let html = '';
  let lastPos = 0;
  for (const t of tokens) {
    if (t.pos > lastPos) html += escapeHtml(source.slice(lastPos, t.pos));
    const cls = tokenClass(t.kind);
    html += `<span class="${cls}">${escapeHtml(t.text)}</span>`;
    lastPos = t.pos + t.text.length;
  }
  if (lastPos < source.length) html += escapeHtml(source.slice(lastPos));
  el.innerHTML = html;
}

function showTokens(tokens) {
  const el = document.getElementById('output-tokens');
  if (!tokens || !tokens.length) { el.innerHTML = '<span style="color:var(--text-dim)">No tokens</span>'; return; }
  let html = '<table class="token-table"><tr><th>Kind</th><th>Text</th><th>Pos</th></tr>';
  for (const t of tokens) {
    const cls = tokenClass(t.kind);
    html += `<tr><td class="token-${cls}">${t.kind}</td><td>${escapeHtml(t.text)}</td><td>${t.pos}</td></tr>`;
  }
  html += '</table>';
  el.innerHTML = html;
}

function showExplain(text) {
  document.getElementById('output-explain').textContent = text || '';
}

function showCompile(val, err) {
  const el = document.getElementById('output-compile');
  if (err) { el.textContent = 'Error: ' + err; return; }
  if (!val) { el.textContent = ''; return; }
  // compile() now returns a JSON string
  el.textContent = JSON.stringify(JSON.parse(val), null, 2);
}

function showAST(val, err) {
  const el = document.getElementById('output-ast');
  if (err) { el.textContent = 'Error: ' + err; return; }
  el.textContent = val ? JSON.stringify(val, null, 2) : '';
}

function showResponse(val) {
  const el = document.getElementById('output-response');
  if (!val) { el.textContent = ''; return; }
  el.textContent = JSON.stringify(val, null, 2);
}

function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// ── Tab switching ──
function switchOutputTab(name) {
  document.querySelectorAll('.output-tabs .tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === name);
  });
  const ids = { explain: 'output-explain', compile: 'output-compile', tokens: 'output-tokens', ast: 'output-ast', response: 'output-response' };
  for (const [k, id] of Object.entries(ids)) {
    document.getElementById(id).style.display = k === name ? 'block' : 'none';
  }
}

function switchEditorTab(name) {
  document.querySelectorAll('.editor-header .tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === name);
  });
  document.getElementById('query-input').style.display = name === 'edit' ? 'block' : 'none';
  document.getElementById('highlight-view').style.display = name === 'highlight' ? 'block' : 'none';
}

// ── Connect ──
async function connectQdrant() {
  const url = document.getElementById('qdrant-url').value.trim();
  const key = document.getElementById('qdrant-key').value.trim();
  if (!url) return;
  client = new Client(url, key || undefined);
  document.getElementById('btn-execute').disabled = !isValid(currentQuery);
  setStatus('✓ Connected', 'ok');
}

// ── Execute ──
async function executeQuery() {
  if (!client) return;
  const q = currentQuery;
  if (!q) return;
  try {
    const result = await client.execute(q);
    showResponse(result);
    switchOutputTab('response');
  } catch (e) {
    showResponse({ error: String(e) });
    switchOutputTab('response');
  }
}

// ── Sample queries ──
const SAMPLES = {
  query: `-- Search with filters and payload selection
QUERY 'vector database optimization' FROM docs
  LIMIT 10 OFFSET 0
  USING HYBRID
  WHERE category = 'tech' AND year >= 2024
  WITH PAYLOAD (include = ['title', 'url'], exclude = ['raw_text'])
  WITH VECTOR ('dense')
  RERANK
  WITH (hnsw_ef = 256)`,
  upsert: `-- Upsert documents with auto-embedding
UPSERT INTO docs VALUES
  {id: 1, text: 'Qdrant is a high-performance vector database', category: 'database', year: 2024},
  {id: 2, text: 'Vector search enables semantic retrieval at scale', category: 'search', year: 2025}
USING HYBRID`,
  ddl: `-- Collection Management
CREATE COLLECTION products HYBRID
  WITH HNSW (m = 32, ef_construct = 100)
  WITH QUANTIZATION (type = 'scalar', quantile = 0.95)

CREATE INDEX ON COLLECTION products FOR category TYPE keyword
CREATE INDEX ON COLLECTION products FOR price TYPE float

SHOW COLLECTIONS`,
  cte: `-- Multi-stage retrieval with CTE prefetches
WITH
  dense AS (QUERY 'vector database performance' USING dense LIMIT 200
            WHERE category = 'tech'),
  sparse AS (QUERY 'vector database performance' USING sparse LIMIT 300)
QUERY 'vector database performance' FROM articles LIMIT 10
  PREFETCH (dense SCORE THRESHOLD 0.6, sparse SCORE THRESHOLD 0.3)
  FUSION RRF
  WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])`
};

// ── Init ──
async function main() {
  await init();
  wasmReady = true;

  const ta = document.getElementById('query-input');
  ta.value = "QUERY 'search' FROM docs LIMIT 10";
  ta.addEventListener('input', debouncedRefresh);

  document.getElementById('btn-connect').addEventListener('click', connectQdrant);
  document.getElementById('btn-execute').addEventListener('click', executeQuery);

  document.querySelectorAll('.output-tabs .tab').forEach(t => {
    t.addEventListener('click', () => switchOutputTab(t.dataset.tab));
  });
  document.querySelectorAll('.editor-header .tab').forEach(t => {
    t.addEventListener('click', () => switchEditorTab(t.dataset.tab));
  });

  document.getElementById('btn-sample-query').addEventListener('click', () => { ta.value = SAMPLES.query; ta.dispatchEvent(new Event('input')); });
  document.getElementById('btn-sample-upsert').addEventListener('click', () => { ta.value = SAMPLES.upsert; ta.dispatchEvent(new Event('input')); });
  document.getElementById('btn-sample-ddl').addEventListener('click', () => { ta.value = SAMPLES.ddl; ta.dispatchEvent(new Event('input')); });
  document.getElementById('btn-sample-cte').addEventListener('click', () => { ta.value = SAMPLES.cte; ta.dispatchEvent(new Event('input')); });

  // Tab key support in textarea
  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      const start = ta.selectionStart, end = ta.selectionEnd;
      ta.value = ta.value.substring(0, start) + '  ' + ta.value.substring(end);
      ta.selectionStart = ta.selectionEnd = start + 2;
      ta.dispatchEvent(new Event('input'));
    }
  });

  await refresh();
}

main().catch(console.error);
