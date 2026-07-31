import * as vscode from "vscode";
import { QQL_KEYWORDS } from "./keywords.generated";

const UNIQUE_KEYWORDS = [...QQL_KEYWORDS];

// ── Snippets ─────────────────────────────────────────────────────
interface QqlCompletion {
  label: string;
  insertText: string;
  detail: string;
}

const SNIPPETS: QqlCompletion[] = [
  {
    label: "QUERY NEAREST",
    insertText: "QUERY TEXT '${1:search term}'\n  FROM ${2:collection} USING ${3:dense}\n  LIMIT ${4:10};",
    detail: "Nearest-neighbor semantic search",
  },
  {
    label: "QUERY HYBRID",
    insertText: "QUERY HYBRID TEXT '${1:search term}'\n  DENSE ${2:dense} SPARSE ${3:sparse}\n  FUSION RRF\n  FROM ${4:collection}\n  LIMIT ${5:10};",
    detail: "Hybrid dense+sparse RRF search",
  },
  {
    label: "QUERY HYBRID DBSF",
    insertText: "QUERY HYBRID TEXT '${1:search term}'\n  DENSE ${2:dense} SPARSE ${3:sparse}\n  FUSION DBSF\n  FROM ${4:collection}\n  LIMIT ${5:10};",
    detail: "Hybrid dense+sparse DBSF search",
  },
  {
    label: "CTE + FUSION",
    insertText: "WITH\n  ${1:dense} AS (QUERY TEXT '${2:search}' USING ${3:dense} LIMIT 100),\n  ${4:sparse} AS (QUERY TEXT '${2:search}' USING ${5:sparse} LIMIT 100)\nQUERY FUSION RRF\n  FROM ${6:collection}\n  PREFETCH (${1:dense}, ${4:sparse})\n  LIMIT ${7:10};",
    detail: "Multi-stage retrieval with CTEs and RRF fusion",
  },
  {
    label: "CTE + RERANK",
    insertText: "WITH ${1:candidates} AS (QUERY TEXT '${2:search}' USING ${3:dense} LIMIT 100)\nQUERY RERANK TEXT '${2:search}' MODEL '${4:reranker}'\n  FROM ${5:collection}\n  USING ${6:colbert}\n  PREFETCH (${1:candidates})\n  LIMIT ${7:10};",
    detail: "Two-stage retrieval with reranking",
  },
  {
    label: "QUERY RECOMMEND",
    insertText: "QUERY RECOMMEND POSITIVE (${1:1}, ${2:2}) NEGATIVE (${3:3})\n  STRATEGY ${4|average_vector,best_score,sum_scores|}\n  FROM ${5:collection}\n  LIMIT ${6:10};",
    detail: "Recommendation from positive/negative examples",
  },
  {
    label: "QUERY MMR",
    insertText: "QUERY MMR TEXT '${1:search}' DIVERSITY ${2:0.5} CANDIDATES ${3:100}\n  FROM ${4:collection} USING ${5:dense}\n  LIMIT ${6:10};",
    detail: "Maximal Marginal Relevance for diversity",
  },
  {
    label: "QUERY FORMULA",
    insertText: "QUERY FORMULA ${1:\\$score * 2 + 0.3 * popularity}\n  DEFAULTS (${2:score} = ${3:0.0})\n  FROM ${4:collection}\n  LIMIT ${5:10};",
    detail: "Formula-based scoring with boost",
  },
  {
    label: "QUERY POINTS",
    insertText: "QUERY POINTS (${1:1}, ${2:2})\n  FROM ${3:collection}\n  WITH PAYLOAD INCLUDE (${4:title})\n  WITH VECTOR false;",
    detail: "Direct point ID retrieval (no LIMIT)",
  },
  {
    label: "QUERY ORDER BY",
    insertText: "QUERY ORDER BY ${1:field} ${2|ASC,DESC|}\n  FROM ${3:collection}\n  LIMIT ${4:10};",
    detail: "Order by payload field",
  },
  {
    label: "QUERY SAMPLE",
    insertText: "QUERY SAMPLE RANDOM\n  FROM ${1:collection}\n  LIMIT ${2:10};",
    detail: "Random point sampling",
  },
  {
    label: "UPSERT INTO",
    insertText: "UPSERT INTO ${1:collection} VALUES\n  {id: ${2:1}, text: '${3:document}'}\n  USING DENSE MODEL '${4:all-MiniLM-L6-v2}';",
    detail: "Upsert points with auto-embedding",
  },
  {
    label: "CREATE COLLECTION",
    insertText: "CREATE COLLECTION ${1:collection} (\n"
      + "  ${2:dense} VECTOR(${3:384}, ${4|COSINE,DOT,EUCLID,MANHATTAN|})\n"
      + ") WITH HNSW (m = ${5:16}, ef_construct = ${6:100});",
    detail: "Create a new collection",
  },
  {
    label: "CREATE COLLECTION HYBRID",
    insertText: "CREATE COLLECTION ${1:collection} (\n"
      + "  ${2:dense} VECTOR(${3:384}, ${4|COSINE,DOT,EUCLID,MANHATTAN|}),\n"
      + "  ${5:sparse} SPARSE\n"
      + ") WITH HNSW (m = ${6:16}, ef_construct = ${7:100});",
    detail: "Create a hybrid (dense+sparse) collection",
  },
  {
    label: "CREATE INDEX",
    insertText: "CREATE INDEX ON COLLECTION ${1:collection}\n  FOR ${2:field}\n  TYPE ${3|keyword,integer,float,geo,text,bool,datetime,uuid|}\n  WITH (${4});",
    detail: "Create a payload index",
  },
  {
    label: "SCROLL",
    insertText: "SCROLL FROM ${1:collection} LIMIT ${2:100};",
    detail: "Scroll through points",
  },
  {
    label: "COUNT",
    insertText: "COUNT FROM ${1:collection} WHERE ${2:status} = '${3:active}';",
    detail: "Count points with filter",
  },
  {
    label: "DELETE",
    insertText: "DELETE FROM ${1:collection} WHERE ${2:status} = '${3:archived}';",
    detail: "Delete points by filter",
  },
  {
    label: "DELETE PAYLOAD",
    insertText: "DELETE PAYLOAD ${1:key1}, ${2:key2} FROM ${3:collection} WHERE ${4:status} = '${5:archived}';",
    detail: "Delete specific payload keys from matching points",
  },
  {
    label: "COUNT EXACT",
    insertText: "COUNT FROM ${1:collection} WHERE ${2:active} = true WITH (exact = true);",
    detail: "Exact point count (not approximate)",
  },
  {
    label: "CROSS RERANK",
    insertText: "WITH ${1:candidates} AS (QUERY TEXT '${2:search}' USING ${3:dense} LIMIT 100)\nQUERY CROSS RERANK TEXT '${2:search}' MODEL '${4:cross-encoder}' ON FIELD ${5:body}\n  FROM ${6:collection}\n  PREFETCH (${1:candidates})\n  LIMIT ${7:10};",
    detail: "Cross-encoder pair scoring over prefetch candidates",
  },
  {
    label: "QUERY IMAGE",
    insertText: "QUERY IMAGE '${1:/path/to/image.jpg}' MODEL '${2:clip-vit}'\n  FROM ${3:collection} USING ${4:image}\n  LIMIT ${5:10};",
    detail: "CLIP vision embedding search",
  },
  {
    label: "USING HYBRID (tail-form)",
    insertText: "QUERY TEXT '${1:search}' FROM ${2:collection}\n  USING HYBRID DENSE ${3:dense} SPARSE ${4:sparse} FUSION RRF\n  LIMIT ${5:10};",
    detail: "Hybrid search using tail-form USING HYBRID",
  },
  {
    label: "QUERY AS MULTI",
    insertText: "QUERY TEXT '${1:search}' FROM ${2:collection}\n  USING ${3:colbert} AS MULTI\n  LIMIT ${4:10};",
    detail: "ColBERT multivector nearest search",
  },
  {
    label: "QUERY ACORN",
    insertText: "QUERY TEXT '${1:search}' FROM ${2:collection} USING ${3:dense}\n  PARAMS (acorn = true, max_selectivity = ${4:0.5})\n  LIMIT ${5:10};",
    detail: "ACORN search with max_selectivity",
  },
  {
    label: "CREATE SHARD KEY",
    insertText: "CREATE SHARD KEY '${1:tenant-a}';",
    detail: "Create a new shard key for multi-tenancy",
  },
  {
    label: "SHARD KEY lifecycle",
    insertText: "CREATE SHARD KEY '${1:tenant}'; SHOW SHARD KEYS; DROP SHARD KEY '${1:tenant}';",
    detail: "Full shard key lifecycle: create, show, drop",
  },
];

// ── Completion Provider ──────────────────────────────────────────

export class QqlCompletionProvider implements vscode.CompletionItemProvider {
  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
    _context: vscode.CompletionContext
  ): vscode.ProviderResult<vscode.CompletionItem[]> {
    const linePrefix = document
      .lineAt(position)
      .text.substring(0, position.character);

    // Skip completion inside strings or comments
    if (isInsideString(linePrefix) || linePrefix.includes("--")) {
      return [];
    }

    const items: vscode.CompletionItem[] = [];

    // Keyword completions
    for (const kw of UNIQUE_KEYWORDS) {
      const item = new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword);
      item.detail = "QQL keyword";
      items.push(item);
    }

    // Snippet completions
    for (const snip of SNIPPETS) {
      const item = new vscode.CompletionItem(snip.label, vscode.CompletionItemKind.Snippet);
      item.detail = snip.detail;
      item.insertText = new vscode.SnippetString(snip.insertText);
      item.documentation = new vscode.MarkdownString(
        `**${snip.label}**\n\n${snip.detail}\n\n\`\`\`qql\n${snip.insertText}\n\`\`\``
      );
      items.push(item);
    }

    return items;
  }
}

/**
 * Check if cursor is inside an unclosed string on the current line.
 * Simple heuristic: odd number of unescaped quotes.
 */
function isInsideString(linePrefix: string): boolean {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < linePrefix.length; i++) {
    const ch = linePrefix[i];
    if (ch === "\\") {
      i++; // skip escaped char
    } else if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
    }
  }
  return inSingle || inDouble;
}
