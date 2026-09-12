import * as vscode from "vscode";
import type { AnalysisService } from "../core/analysis";
import { QQL_KEYWORDS } from "../keywords.generated";

const UNIQUE_KEYWORDS = [...QQL_KEYWORDS];

// ── Snippets ─────────────────────────────────────────────────────
interface QqlSnippet {
  label: string;
  insertText: string;
  detail: string;
  /** Only show when the line prefix matches (case-insensitive startsWith / includes). */
  when?: (ctx: CompletionContext) => boolean;
}

interface CompletionContext {
  linePrefix: string;
  prevWord: string;
  prevPrevWord: string;
  isStart: boolean;
  fullPrefix: string;
}

const SNIPPETS: QqlSnippet[] = [
  {
    label: "QUERY NEAREST",
    insertText: `QUERY TEXT '\${1:search term}'
  FROM \${2:collection} USING \${3:dense}
  LIMIT \${4:10};`,
    detail: "Nearest-neighbor semantic search",
  },
  {
    label: "QUERY HYBRID",
    insertText: `QUERY HYBRID TEXT '\${1:search term}'
  DENSE \${2:dense} SPARSE \${3:sparse}
  FUSION RRF
  FROM \${4:collection}
  LIMIT \${5:10};`,
    detail: "Hybrid dense+sparse RRF search",
  },
  {
    label: "QUERY HYBRID DBSF",
    insertText: `QUERY HYBRID TEXT '\${1:search term}'
  DENSE \${2:dense} SPARSE \${3:sparse}
  FUSION DBSF
  FROM \${4:collection}
  LIMIT \${5:10};`,
    detail: "Hybrid dense+sparse DBSF search",
  },
  {
    label: "CTE + FUSION",
    insertText: `WITH
  \${1:dense} AS (QUERY TEXT '\${2:search}' USING \${3:dense} LIMIT 100),
  \${4:sparse} AS (QUERY TEXT '\${2:search}' USING \${5:sparse} LIMIT 100)
QUERY FUSION RRF
  FROM \${6:collection}
  PREFETCH (\${1:dense}, \${4:sparse})
  LIMIT \${7:10};`,
    detail: "Multi-stage retrieval with CTEs and RRF fusion",
  },
  {
    label: "CTE + RERANK",
    insertText: `WITH \${1:candidates} AS (QUERY TEXT '\${2:search}' USING \${3:dense} LIMIT 100)
QUERY RERANK TEXT '\${2:search}' MODEL '\${4:reranker}'
  FROM \${5:collection}
  USING \${6:colbert}
  PREFETCH (\${1:candidates})
  LIMIT \${7:10};`,
    detail: "Two-stage retrieval with reranking",
  },
  {
    label: "QUERY RECOMMEND",
    insertText: `QUERY RECOMMEND POSITIVE (\${1:1}, \${2:2}) NEGATIVE (\${3:3})
  STRATEGY \${4|average_vector,best_score,sum_scores|}
  FROM \${5:collection}
  LIMIT \${6:10};`,
    detail: "Recommendation from positive/negative examples",
  },
  {
    label: "QUERY MMR",
    insertText: `QUERY MMR TEXT '\${1:search}' DIVERSITY \${2:0.5} CANDIDATES \${3:100}
  FROM \${4:collection} USING \${5:dense}
  LIMIT \${6:10};`,
    detail: "Maximal Marginal Relevance for diversity",
  },
  {
    label: "QUERY FORMULA",
    insertText: `QUERY FORMULA \${1:\\$score * 2 + 0.3 * popularity}
  DEFAULTS (\${2:score} = \${3:0.0})
  FROM \${4:collection}
  LIMIT \${5:10};`,
    detail: "Formula-based scoring with boost",
  },
  {
    label: "QUERY POINTS",
    insertText: `QUERY POINTS (\${1:1}, \${2:2})
  FROM \${3:collection}
  WITH PAYLOAD INCLUDE (\${4:title})
  WITH VECTOR false;`,
    detail: "Direct point ID retrieval (no LIMIT)",
  },
  {
    label: "QUERY ORDER BY",
    insertText: `QUERY ORDER BY \${1:field} \${2|ASC,DESC|}
  FROM \${3:collection}
  LIMIT \${4:10};`,
    detail: "Order by payload field",
  },
  {
    label: "QUERY SAMPLE",
    insertText: `QUERY SAMPLE RANDOM
  FROM \${1:collection}
  LIMIT \${2:10};`,
    detail: "Random point sampling",
  },
  {
    label: "UPSERT INTO",
    insertText: `UPSERT INTO \${1:collection} VALUES
  {id: \${2:1}, text: '\${3:document}'}
  USING DENSE MODEL '\${4:all-MiniLM-L6-v2}'
  WAIT \${5|true,false|};`,
    detail: "Upsert points with auto-embedding (durable with WAIT)",
  },
  {
    label: "CREATE COLLECTION",
    insertText: `CREATE COLLECTION \${1:collection} (
  \${2:dense} VECTOR(\${3:384}, \${4|COSINE,DOT,EUCLID,MANHATTAN|})
) WITH HNSW (m = \${5:16}, ef_construct = \${6:100});`,
    detail: "Create a new collection",
  },
  {
    label: "CREATE COLLECTION HYBRID",
    insertText: `CREATE COLLECTION \${1:collection} (
  \${2:dense} VECTOR(\${3:384}, \${4|COSINE,DOT,EUCLID,MANHATTAN|}),
  \${5:sparse} SPARSE
) WITH HNSW (m = \${6:16}, ef_construct = \${7:100});`,
    detail: "Create a hybrid (dense+sparse) collection",
  },
  {
    label: "CREATE INDEX",
    insertText: `CREATE INDEX ON COLLECTION \${1:collection}
  FOR \${2:field}
  TYPE \${3|keyword,integer,float,geo,text,bool,datetime,uuid|}
  WITH (\${4})
  WAIT \${5|true,false|};`,
    detail: "Create a payload index (durable with WAIT)",
  },
  {
    label: "SCROLL",
    insertText: `SCROLL FROM \${1:collection} LIMIT \${2:100};`,
    detail: "Scroll through points",
  },
  {
    label: "COUNT",
    insertText: `COUNT FROM \${1:collection} WHERE \${2:status} = '\${3:active}';`,
    detail: "Count points with filter",
  },
  {
    label: "FACET",
    insertText: `FACET \${1:field}
  FROM \${2:collection}
  WHERE \${3:filter}
  LIMIT \${4:10}
  EXACT \${5:true};`,
    detail: "Compute value distribution for payload field",
  },
  {
    label: "DELETE",
    insertText: `DELETE FROM \${1:collection} WHERE \${2:status} = '\${3:archived}' WAIT \${4|true,false|};`,
    detail: "Delete points by filter (durable with WAIT)",
  },
  {
    label: "DELETE PAYLOAD",
    insertText: `DELETE PAYLOAD \${1:key1}, \${2:key2} FROM \${3:collection} WHERE \${4:status} = '\${5:archived}';`,
    detail: "Delete specific payload keys from matching points",
  },
  {
    label: "COUNT EXACT",
    insertText: `COUNT FROM \${1:collection} WHERE \${2:active} = true WITH (exact = true);`,
    detail: "Exact point count (not approximate)",
  },
  {
    label: "CROSS RERANK",
    insertText: `WITH \${1:candidates} AS (QUERY TEXT '\${2:search}' USING \${3:dense} LIMIT 100)
QUERY CROSS RERANK TEXT '\${2:search}' MODEL '\${4:cross-encoder}' ON FIELD \${5:body}
  FROM \${6:collection}
  PREFETCH (\${1:candidates})
  LIMIT \${7:10};`,
    detail: "Cross-encoder pair scoring over prefetch candidates",
  },
  {
    label: "QUERY IMAGE",
    insertText: `QUERY IMAGE '\${1:/path/to/image.jpg}' MODEL '\${2:clip-vit}'
  FROM \${3:collection} USING \${4:image}
  LIMIT \${5:10};`,
    detail: "CLIP vision embedding search",
  },
  {
    label: "USING HYBRID (tail-form)",
    insertText: `QUERY TEXT '\${1:search}' FROM \${2:collection}
  USING HYBRID DENSE \${3:dense} SPARSE \${4:sparse} FUSION RRF
  LIMIT \${5:10};`,
    detail: "Hybrid search using tail-form USING HYBRID",
  },
  {
    label: "QUERY AS MULTI",
    insertText: `QUERY TEXT '\${1:search}' FROM \${2:collection}
  USING \${3:colbert} AS MULTI
  LIMIT \${4:10};`,
    detail: "ColBERT multivector nearest search",
  },
  {
    label: "QUERY ACORN",
    insertText: `QUERY TEXT '\${1:search}' FROM \${2:collection} USING \${3:dense}
  PARAMS (acorn = true, max_selectivity = \${4:0.5})
  LIMIT \${5:10};`,
    detail: "ACORN search with max_selectivity",
  },
  {
    label: "CREATE SHARD KEY",
    insertText: `CREATE SHARD KEY '\${1:tenant}' ON COLLECTION \${2:collection} WITH (shards_number = \${3:2});`,
    detail: "Create a new shard key for multi-tenancy",
  },
  {
    label: "SHARD KEY lifecycle",
    insertText: `CREATE SHARD KEY '\${1:tenant}' ON COLLECTION \${2:tenants} WITH (shards_number = 2);
SHOW SHARD KEYS ON COLLECTION \${2:tenants};
DROP SHARD KEY '\${1:tenant}' ON COLLECTION \${2:tenants};`,
    detail: "Full shard key lifecycle: create, show, drop",
  },
  {
    label: "GROUP BY LOOKUP",
    insertText: `QUERY TEXT '\${1:search}' FROM \${2:collection}
  USING \${3:dense}
  GROUP BY '\${4:author_id}' SIZE \${5:5} LOOKUP FROM \${6:authors}
  LIMIT \${7:20};`,
    detail: "Grouped search with cross-collection lookup",
  },
  {
    label: "FORMULA DECAY",
    insertText: `WITH \${1:candidates} AS (QUERY TEXT '\${2:search}' FROM \${3:collection} USING \${4:dense} LIMIT 100)
QUERY FORMULA (score * GAUSS_DECAY(GEO_DISTANCE(\${5:48.8566}, \${6:2.3522}, location), 0.0, \${7:5000.0}, 0.5))
  DEFAULTS (location = {lat: \${5:48.8566}, lon: \${6:2.3522}})
  FROM \${3:collection}
  PREFETCH (\${1:candidates})
  LIMIT \${8:10};`,
    detail: "Geo-distance Gaussian decay scoring",
  },
  {
    label: "ALTER VECTOR DIFF",
    insertText: `ALTER COLLECTION \${1:collection}
  WITH VECTOR \${2:dense} (HNSW (m = \${3:32}), QUANTIZATION (type = '\${4:scalar}'));`,
    detail: "Per-vector ALTER diff (HNSW / QUANTIZATION / VECTOR)",
  },
  {
    label: "ALTER SPARSE DIFF",
    insertText: `ALTER COLLECTION \${1:collection}
  WITH SPARSE \${2:bm25} (SPARSE (modifier = '\${3:idf}', full_scan_threshold = \${4:5000}));`,
    detail: "Per-sparse-vector ALTER diff",
  },
  {
    label: "ALTER HNSW",
    insertText: `ALTER COLLECTION \${1:collection} WITH HNSW (m = \${2:32});`,
    detail: "Patch collection HNSW params",
  },
  {
    label: "SHARD NUMERIC",
    insertText: `QUERY TEXT '\${1:search}'
  FROM \${2:collection} USING \${3:dense}
  WHERE \${4:tenant_id} = '\${5:acme}'
  SHARD \${6:101}
  LIMIT \${7:10};`,
    detail: "Typed shard routing to a numeric partition",
  },
  {
    label: "QUERY WITH PARAMS",
    insertText: `-- qql-params: {"q": "\${1:search term}", "limit": \${2:10}}
QUERY TEXT :q
  FROM \${3:collection} USING \${4:dense}
  LIMIT :limit;`,
    detail: "Placeholder query with a qql-params header",
  },
  {
    label: "CREATE SHARD KEY NUMERIC",
    insertText: `CREATE SHARD KEY \${1:101} ON COLLECTION \${2:collection} WITH (shards_number = \${3:2});`,
    detail: "Create a numeric shard key for multi-tenancy",
  },
];

// Contextual follow-ups after a keyword
const AFTER: Record<
  string,
  Array<{ label: string; kind?: vscode.CompletionItemKind; detail?: string }>
> = {
  QUERY: [
    { label: "TEXT", detail: "Nearest from text" },
    { label: "HYBRID", detail: "Dense + sparse fusion" },
    { label: "RECOMMEND", detail: "Positive/negative examples" },
    { label: "DISCOVER", detail: "Target + context pairs" },
    { label: "FORMULA", detail: "Score shaping" },
    { label: "FUSION", detail: "Merge prefetches" },
    { label: "RERANK", detail: "Late-interaction rerank" },
    { label: "CROSS", detail: "CROSS RERANK pair scorer" },
    { label: "MMR", detail: "Diversity rerank" },
    { label: "POINTS", detail: "Fetch by ID" },
    { label: "ORDER", detail: "ORDER BY payload" },
    { label: "SAMPLE", detail: "SAMPLE RANDOM" },
    { label: "IMAGE", detail: "Vision embedding" },
    { label: "NEAREST", detail: "Explicit nearest" },
    { label: "CONTEXT", detail: "Context query" },
    { label: "RELEVANCE", detail: "Relevance feedback" },
  ],
  FUSION: [
    { label: "RRF", detail: "Reciprocal Rank Fusion" },
    { label: "DBSF", detail: "Distribution-Based Score Fusion" },
  ],
  STRATEGY: [{ label: "average_vector" }, { label: "best_score" }, { label: "sum_scores" }],
  TYPE: [
    { label: "keyword" },
    { label: "integer" },
    { label: "float" },
    { label: "geo" },
    { label: "text" },
    { label: "bool" },
    { label: "datetime" },
    { label: "uuid" },
  ],
  USING: [
    { label: "dense", detail: "Named dense vector" },
    { label: "sparse", detail: "Named sparse vector" },
    { label: "HYBRID", detail: "Tail-form hybrid" },
    { label: "colbert", detail: "Multivector (AS MULTI)" },
  ],
  ORDER: [{ label: "BY" }],
  SAMPLE: [{ label: "RANDOM" }],
  CROSS: [{ label: "RERANK" }],
  CREATE: [{ label: "COLLECTION" }, { label: "INDEX" }, { label: "SHARD" }],
  DROP: [{ label: "COLLECTION" }, { label: "INDEX" }, { label: "SHARD" }],
  SHOW: [
    { label: "COLLECTIONS" },
    { label: "COLLECTION" },
    { label: "SHARD" },
    { label: "QUOTAS" },
  ],
  ALTER: [{ label: "COLLECTION" }],
  SET: [{ label: "QUOTA" }],
  WAIT: [{ label: "true" }, { label: "false" }],
  DELETE: [{ label: "FROM" }, { label: "PAYLOAD" }, { label: "VECTOR" }],
  CLEAR: [{ label: "PAYLOAD" }],
  UPDATE: [{ label: "VECTOR" }, { label: "PAYLOAD" }],
  WITH: [
    { label: "PAYLOAD", detail: "Projection" },
    { label: "VECTOR", detail: "Return vectors / per-vector diff" },
    { label: "SPARSE", detail: "Per-sparse-vector diff" },
    { label: "HNSW", detail: "DDL HNSW config" },
    { label: "QUANTIZATION", detail: "DDL quantization config" },
    { label: "OPTIMIZERS", detail: "DDL optimizer config" },
    { label: "PARAMS", detail: "DDL / request params" },
    { label: "MULTIVECTOR", detail: "ColBERT shape" },
  ],
  SCORE: [{ label: "THRESHOLD" }],
  GROUP: [{ label: "BY" }],
  SHARD: [{ label: "KEY" }, { label: "KEYS" }],
  VECTOR: [{ label: "COSINE" }, { label: "DOT" }, { label: "EUCLID" }, { label: "MANHATTAN" }],
  CONSISTENCY: [{ label: "majority" }, { label: "quorum" }, { label: "all" }],
  IS: [{ label: "NULL" }, { label: "NOT" }, { label: "EMPTY" }],
  MATCH: [{ label: "ANY" }],
  ASC: [],
  DESC: [],
};

const STATEMENT_STARTERS = [
  "QUERY",
  "WITH",
  "SCROLL",
  "COUNT",
  "FACET",
  "UPSERT",
  "DELETE",
  "CREATE",
  "DROP",
  "ALTER",
  "SHOW",
  "SET",
  "CLEAR",
  "UPDATE",
];

const CLAUSE_KEYWORDS = [
  "FROM",
  "WHERE",
  "LIMIT",
  "OFFSET",
  "USING",
  "PREFETCH",
  "PARAMS",
  "GROUP",
  "WITH",
  "SCORE",
  "FILTER",
  "SHARD",
  "MODEL",
  "ON",
  "FOR",
  "TYPE",
  "INTO",
  "VALUES",
  "SET",
  "STRATEGY",
  "POSITIVE",
  "NEGATIVE",
  "TARGET",
  "CONTEXT",
  "DEFAULTS",
  "DIVERSITY",
  "CANDIDATES",
  "LOOKUP",
  "SIZE",
  "FIELD",
  "WAIT",
];

// ── Completion Provider ──────────────────────────────────────────

export class QqlCompletionProvider implements vscode.CompletionItemProvider {
  constructor(private readonly analysis?: AnalysisService) {}

  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
    _context: vscode.CompletionContext
  ): vscode.ProviderResult<vscode.CompletionList> {
    const line = document.lineAt(position).text;
    const linePrefix = line.substring(0, position.character);

    if (isInsideString(linePrefix) || isInsideComment(linePrefix)) {
      return new vscode.CompletionList([], true);
    }

    const ctx = buildContext(document, position, linePrefix);
    const items: vscode.CompletionItem[] = [];
    const seen = new Set<string>();

    const add = (item: vscode.CompletionItem, boost = 0) => {
      const key = item.label.toString().toUpperCase();
      if (seen.has(key)) return;
      seen.add(key);
      if (boost) item.sortText = `${String(1000 - boost).padStart(4, "0")}_${key}`;
      items.push(item);
    };

    // 1) Contextual follow-ups (highest priority)
    const after = AFTER[ctx.prevWord.toUpperCase()];
    if (after && after.length > 0) {
      for (const entry of after) {
        const item = new vscode.CompletionItem(
          entry.label,
          entry.kind ?? vscode.CompletionItemKind.Keyword
        );
        item.detail = entry.detail ?? "QQL";
        item.preselect = true;
        add(item, 900);
      }
    }

    // After FROM / INTO / COLLECTION — suggest collection names seen in the file
    if (["FROM", "INTO", "COLLECTION"].includes(ctx.prevWord.toUpperCase())) {
      for (const name of collectIdentifiers(document, this.analysis)) {
        const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Variable);
        item.detail = "collection (from file)";
        add(item, 850);
      }
    }

    // After PREFETCH ( — suggest CTE names
    if (
      ctx.prevWord.toUpperCase() === "PREFETCH" ||
      (ctx.prevWord === "(" && ctx.prevPrevWord.toUpperCase() === "PREFETCH")
    ) {
      for (const name of collectCteNames(document, this.analysis)) {
        const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Reference);
        item.detail = "CTE";
        add(item, 880);
      }
    }

    // 2) Statement starters at beginning of statement
    if (ctx.isStart) {
      for (const kw of STATEMENT_STARTERS) {
        const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
        item.detail = "statement";
        add(item, 700);
      }
    }

    // 3) Common clauses mid-statement
    if (!ctx.isStart) {
      for (const kw of CLAUSE_KEYWORDS) {
        const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
        item.detail = "clause";
        add(item, 400);
      }
    }

    // 4) Snippets (boost at statement start)
    for (const snip of SNIPPETS) {
      if (snip.when && !snip.when(ctx)) continue;
      const item = new vscode.CompletionItem(snip.label, vscode.CompletionItemKind.Snippet);
      item.detail = snip.detail;
      item.insertText = new vscode.SnippetString(snip.insertText);
      item.documentation = new vscode.MarkdownString(
        `**${snip.label}**\n\n${snip.detail}\n\n\`\`\`qql\n${snip.insertText}\n\`\`\``
      );
      add(item, ctx.isStart ? 600 : 200);
    }

    // 5) Full keyword list (lowest priority — still available for filtering)
    for (const kw of UNIQUE_KEYWORDS) {
      const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
      item.detail = "QQL keyword";
      add(item, 100);
    }

    return new vscode.CompletionList(items, /* isIncomplete */ false);
  }
}

function buildContext(
  document: vscode.TextDocument,
  position: vscode.Position,
  linePrefix: string
): CompletionContext {
  const fullPrefix = document.getText(new vscode.Range(new vscode.Position(0, 0), position));

  // Words on current line (ignore trailing partial word)
  const trimmed = linePrefix.replace(/[A-Za-z0-9_]*$/, "").trimEnd();
  const words = trimmed.split(/[\s,()[\]{}]+/).filter(Boolean);
  const prevWord = words[words.length - 1] ?? "";
  const prevPrevWord = words[words.length - 2] ?? "";

  // Start of statement: beginning of doc, after `;`, or only whitespace on line after blank
  const before = fullPrefix.replace(/[A-Za-z0-9_]*$/, "");
  const isStart = /^\s*$/.test(before) || /;\s*$/.test(before) || /^\s*$/.test(trimmed);

  return { linePrefix, prevWord, prevPrevWord, isStart, fullPrefix };
}

function collectIdentifiers(document: vscode.TextDocument, analysis?: AnalysisService): string[] {
  const names = new Set<string>();
  const cached = analysis?.get(document.uri);
  if (cached) {
    for (const _stmt of cached.statements) {
      // FROM / INTO identifiers via tokens
    }
    for (const tok of cached.result.tokens) {
      if (tok.kind === "IDENTIFIER" && tok.text.length > 1) {
        names.add(tok.text);
      }
    }
  } else {
    const text = document.getText();
    for (const m of text.matchAll(/\b(?:FROM|INTO|COLLECTION)\s+([A-Za-z_][A-Za-z0-9_]*)/gi)) {
      names.add(m[1]);
    }
  }
  return [...names].sort();
}

function collectCteNames(document: vscode.TextDocument, analysis?: AnalysisService): string[] {
  const names = new Set<string>();
  const cached = analysis?.get(document.uri);
  const text = cached?.source ?? document.getText();
  for (const m of text.matchAll(/\b(?:WITH|,)\s*([A-Za-z_][A-Za-z0-9_]*)\s+AS\s*\(/gi)) {
    names.add(m[1]);
  }
  return [...names].sort();
}

function isInsideString(linePrefix: string): boolean {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < linePrefix.length; i++) {
    const ch = linePrefix[i];
    if (ch === "\\") {
      i++;
    } else if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
    }
  }
  return inSingle || inDouble;
}

function isInsideComment(linePrefix: string): boolean {
  // Line comment: -- outside of strings
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < linePrefix.length; i++) {
    const ch = linePrefix[i];
    if (ch === "\\") {
      i++;
    } else if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
    } else if (ch === "-" && linePrefix[i + 1] === "-" && !inSingle && !inDouble) {
      return true;
    }
  }
  return false;
}
