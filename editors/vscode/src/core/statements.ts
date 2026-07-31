import type { CompiledRoute, QqlToken, StatementSpan, WasmAnalyzeResult } from "./types";
import { sliceByByteOffsets, utf8ByteLength } from "./utf8";

/** Keywords that begin a top-level statement (depth 0). */
const STATEMENT_STARTERS = new Set([
  "QUERY",
  "SCROLL",
  "COUNT",
  "UPSERT",
  "DELETE",
  "CREATE",
  "DROP",
  "ALTER",
  "SHOW",
  "CLEAR",
  "UPDATE",
  "WITH",
]);

/**
 * Split a document into top-level statement spans using lexer tokens.
 * Handles nested parentheses (CTEs) and multi-statement scripts.
 */
export function extractStatementSpans(source: string, result: WasmAnalyzeResult): StatementSpan[] {
  const tokens = result.tokens ?? [];
  if (tokens.length === 0) {
    // Fallback: split on top-level semicolons by a simple scan
    return fallbackSplit(source, result);
  }

  const spans: StatementSpan[] = [];
  let depth = 0;
  let stmtStart: number | null = null;
  let stmtKind = "STATEMENT";
  let pendingCreate = false;
  let pendingDrop = false;
  let pendingShow = false;
  let pendingDelete = false;
  let pendingClear = false;
  let pendingUpdate = false;
  let pendingAlter = false;

  // Token pos/end are UTF-8 byte offsets — never use them as JS string indices.
  const flush = (endByte: number) => {
    if (stmtStart === null) return;
    const text = sliceByByteOffsets(source, stmtStart, endByte).trim();
    if (!text) {
      stmtStart = null;
      return;
    }
    const label = buildLabel(stmtKind, text, spans.length);
    spans.push({
      index: spans.length,
      start: stmtStart,
      end: endByte,
      kind: stmtKind,
      label,
      // Correctly decoded statement text for explain/compile
      source: text,
    });
    stmtStart = null;
    stmtKind = "STATEMENT";
    pendingCreate = pendingDrop = pendingShow = pendingDelete = false;
    pendingClear = pendingUpdate = pendingAlter = false;
  };

  for (const tok of tokens) {
    const kind = tok.kind.toUpperCase();

    if (kind === "LPAREN" || kind === "LBRACE" || kind === "LBRACKET") {
      depth++;
      continue;
    }
    if (kind === "RPAREN" || kind === "RBRACE" || kind === "RBRACKET") {
      depth = Math.max(0, depth - 1);
      continue;
    }

    if (depth === 0 && kind === "SEMICOLON") {
      flush(tok.end);
      continue;
    }

    if (depth === 0 && STATEMENT_STARTERS.has(kind) && stmtStart === null) {
      stmtStart = tok.pos;
      stmtKind = classifyStarter(kind, tok, tokens);
      pendingCreate = kind === "CREATE";
      pendingDrop = kind === "DROP";
      pendingShow = kind === "SHOW";
      pendingDelete = kind === "DELETE";
      pendingClear = kind === "CLEAR";
      pendingUpdate = kind === "UPDATE";
      pendingAlter = kind === "ALTER";
      continue;
    }

    // Refine multi-word starters: CREATE COLLECTION / CREATE INDEX / CREATE SHARD KEY
    if (depth === 0 && stmtStart !== null) {
      if (pendingCreate) {
        if (kind === "COLLECTION") stmtKind = "CREATE COLLECTION";
        else if (kind === "INDEX") stmtKind = "CREATE INDEX";
        else if (kind === "SHARD") stmtKind = "CREATE SHARD KEY";
        pendingCreate = false;
      } else if (pendingDrop) {
        if (kind === "COLLECTION") stmtKind = "DROP COLLECTION";
        else if (kind === "INDEX") stmtKind = "DROP INDEX";
        else if (kind === "SHARD") stmtKind = "DROP SHARD KEY";
        pendingDrop = false;
      } else if (pendingShow) {
        if (kind === "COLLECTIONS") stmtKind = "SHOW COLLECTIONS";
        else if (kind === "COLLECTION") stmtKind = "SHOW COLLECTION";
        else if (kind === "SHARD") stmtKind = "SHOW SHARD KEYS";
        pendingShow = false;
      } else if (pendingDelete) {
        if (kind === "PAYLOAD") stmtKind = "DELETE PAYLOAD";
        else if (kind === "VECTOR") stmtKind = "DELETE VECTOR";
        else if (kind === "FROM") stmtKind = "DELETE";
        pendingDelete = false;
      } else if (pendingClear) {
        if (kind === "PAYLOAD") stmtKind = "CLEAR PAYLOAD";
        pendingClear = false;
      } else if (pendingUpdate) {
        if (kind === "VECTOR") stmtKind = "UPDATE VECTOR";
        else if (kind === "PAYLOAD" || kind === "SET") stmtKind = "UPDATE PAYLOAD";
        pendingUpdate = false;
      } else if (pendingAlter) {
        if (kind === "COLLECTION") stmtKind = "ALTER COLLECTION";
        pendingAlter = false;
      } else if (stmtKind === "QUERY" || stmtKind === "WITH") {
        // Refine QUERY mode from the expression keyword
        if (
          [
            "HYBRID",
            "RECOMMEND",
            "DISCOVER",
            "FORMULA",
            "POINTS",
            "ORDER",
            "SAMPLE",
            "MMR",
            "FUSION",
            "RERANK",
            "CONTEXT",
            "RELEVANCE",
            "IMAGE",
            "CROSS",
            "NEAREST",
          ].includes(kind)
        ) {
          if (kind === "CROSS") stmtKind = "QUERY CROSS RERANK";
          else if (kind === "ORDER") stmtKind = "QUERY ORDER BY";
          else if (kind === "SAMPLE") stmtKind = "QUERY SAMPLE";
          else if (kind === "RELEVANCE") stmtKind = "QUERY RELEVANCE FEEDBACK";
          else if (kind === "POINTS") stmtKind = "QUERY POINTS";
          else stmtKind = `QUERY ${kind}`;
        } else if (kind === "TEXT" && (stmtKind === "QUERY" || stmtKind === "WITH")) {
          stmtKind = "QUERY TEXT";
        }
      }
    }
  }

  // Trailing statement without semicolon (end is UTF-8 byte length, not JS length)
  if (stmtStart !== null) {
    flush(utf8ByteLength(source));
  }

  // Attach routes by index when available
  if (result.routes && result.routes.length > 0) {
    for (let i = 0; i < spans.length && i < result.routes.length; i++) {
      spans[i].route = result.routes[i];
    }
  } else if (result.route && spans.length === 1) {
    spans[0].route = result.route;
  }

  return spans;
}

function classifyStarter(kind: string, _tok: QqlToken, _tokens: QqlToken[]): string {
  return kind;
}

function buildLabel(kind: string, text: string, index: number): string {
  const collection = extractCollection(text);
  const base = kind === "WITH" ? "QUERY (CTE)" : kind;
  if (collection) return `${base} · ${collection}`;
  return `${base} #${index + 1}`;
}

/** Best-effort collection name extraction from statement source. */
export function extractCollection(source: string): string | undefined {
  // FROM <ident>  or  INTO <ident>  or  COLLECTION <ident>  or  ON COLLECTION <ident>
  const patterns = [
    /\bFROM\s+([A-Za-z_][A-Za-z0-9_]*)/i,
    /\bINTO\s+([A-Za-z_][A-Za-z0-9_]*)/i,
    /\bON\s+COLLECTION\s+([A-Za-z_][A-Za-z0-9_]*)/i,
    /\bCOLLECTION\s+([A-Za-z_][A-Za-z0-9_]*)/i,
  ];
  for (const re of patterns) {
    const m = source.match(re);
    if (m) return m[1];
  }
  return undefined;
}

/**
 * Extract CTE names from a WITH … AS ( … ) header for go-to-definition.
 * Returns { name, start, end } in byte offsets relative to the full source
 * when `baseOffset` is provided (default 0).
 */
export function extractCteDefinitions(
  _source: string,
  tokens: QqlToken[],
  baseOffset = 0
): Array<{ name: string; start: number; end: number }> {
  const ctes: Array<{ name: string; start: number; end: number }> = [];
  let i = 0;
  // Look for WITH <name> AS (  or  , <name> AS (
  while (i < tokens.length) {
    const tok = tokens[i];
    const kind = tok.kind.toUpperCase();
    if ((kind === "WITH" || kind === "COMMA") && i + 2 < tokens.length) {
      const nameTok = tokens[i + 1];
      const asTok = tokens[i + 2];
      if (
        (nameTok.kind === "IDENTIFIER" || isKeywordAsIdent(nameTok.kind)) &&
        asTok.kind.toUpperCase() === "AS"
      ) {
        ctes.push({
          name: nameTok.text,
          start: baseOffset + nameTok.pos,
          end: baseOffset + nameTok.end,
        });
      }
    }
    i++;
  }
  return ctes;
}

/** Token kinds that can never be a CTE name. */
const NON_IDENT_KINDS = new Set([
  "AS",
  "LPAREN",
  "RPAREN",
  "LBRACE",
  "RBRACE",
  "LBRACKET",
  "RBRACKET",
  "COMMA",
  "SEMICOLON",
  "STRING",
  "INTEGER",
  "FLOAT",
  "COLON",
  "EQUALS",
  "NOT_EQUALS",
  "LT",
  "GT",
  "LTE",
  "GTE",
  "PLUS",
  "MINUS",
  "STAR",
  "SLASH",
]);

function isKeywordAsIdent(kind: string): boolean {
  // CTE names may be tokenized as keywords (e.g. dense, sparse, relevance)
  return !NON_IDENT_KINDS.has(kind.toUpperCase());
}

function fallbackSplit(source: string, result: WasmAnalyzeResult): StatementSpan[] {
  const parts: StatementSpan[] = [];
  let start = 0;
  let depth = 0;
  let inSingle = false;
  let inDouble = false;

  for (let i = 0; i < source.length; i++) {
    const ch = source[i];
    if (ch === "\\" && (inSingle || inDouble)) {
      i++;
      continue;
    }
    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
      continue;
    }
    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
      continue;
    }
    if (inSingle || inDouble) continue;

    if (ch === "(" || ch === "{" || ch === "[") depth++;
    else if (ch === ")" || ch === "}" || ch === "]") depth = Math.max(0, depth - 1);
    else if (ch === ";" && depth === 0) {
      const text = source.slice(start, i + 1).trim();
      if (text) {
        const kind = guessKind(text);
        parts.push({
          index: parts.length,
          start,
          end: i + 1,
          kind,
          label: buildLabel(kind, text, parts.length),
          source: source.slice(start, i + 1),
          route: result.routes?.[parts.length],
        });
      }
      start = i + 1;
    }
  }
  const tail = source.slice(start).trim();
  if (tail) {
    const kind = guessKind(tail);
    parts.push({
      index: parts.length,
      start,
      end: source.length,
      kind,
      label: buildLabel(kind, tail, parts.length),
      source: source.slice(start),
      route: result.routes?.[parts.length],
    });
  }
  return parts;
}

function guessKind(text: string): string {
  const m = text.match(
    /^\s*(WITH|QUERY|SCROLL|COUNT|UPSERT|DELETE|CREATE|DROP|ALTER|SHOW|CLEAR|UPDATE)\b/i
  );
  return m ? m[1].toUpperCase() : "STATEMENT";
}

/** Find the statement that contains a given byte offset. */
export function statementAtOffset(
  spans: StatementSpan[],
  offset: number
): StatementSpan | undefined {
  return spans.find((s) => offset >= s.start && offset <= s.end);
}

/** Build a curl snippet from a compiled route. */
export function routeToCurl(route: CompiledRoute, baseUrl = "http://localhost:6333"): string {
  const method = (route.method ?? "POST").toUpperCase();
  const path = route.path ?? "/";
  const url = `${baseUrl.replace(/\/$/, "")}${path.startsWith("/") ? path : `/${path}`}`;
  const body = route.payload != null ? JSON.stringify(route.payload, null, 2) : undefined;

  if (method === "GET" || method === "DELETE") {
    return `curl -X ${method} '${url}' \\\n  -H 'Content-Type: application/json'`;
  }
  return (
    `curl -X ${method} '${url}' \\\n` +
    `  -H 'Content-Type: application/json' \\\n` +
    `  -d '${(body ?? "{}").replace(/'/g, `'\\''`)}'`
  );
}
