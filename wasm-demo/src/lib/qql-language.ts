import { StreamLanguage, type StreamParser } from "@codemirror/language"
import { tags as t } from "@lezer/highlight"

/**
 * Lightweight CodeMirror stream language for QQL.
 * Keywords mirror qql-core TokenKind / gen_keywords! — highlighting only.
 */
const KEYWORDS = new Set(
  [
    "UPSERT",
    "INTO",
    "COLLECTION",
    "VALUES",
    "USING",
    "MODEL",
    "HYBRID",
    "DENSE",
    "SPARSE",
    "RERANK",
    "EXACT",
    "WITH",
    "AS",
    "ACORN",
    "QUANTIZE",
    "SCALAR",
    "BINARY",
    "PRODUCT",
    "TURBO",
    "BITS",
    "QUANTILE",
    "ALWAYS",
    "RAM",
    "HNSW",
    "OPTIMIZERS",
    "PARAMS",
    "DISABLED",
    "CREATE",
    "ALTER",
    "DROP",
    "SHOW",
    "COLLECTIONS",
    "SCROLL",
    "RECOMMEND",
    "LIMIT",
    "GROUP",
    "BY",
    "GROUP_SIZE",
    "STRATEGY",
    "DELETE",
    "UPDATE",
    "SET",
    "OFFSET",
    "SCORE",
    "THRESHOLD",
    "LOOKUP",
    "VECTOR",
    "PAYLOAD",
    "FROM",
    "WHERE",
    "ID",
    "INDEX",
    "ON",
    "FOR",
    "TYPE",
    "AND",
    "OR",
    "NOT",
    "IN",
    "BETWEEN",
    "IS",
    "NULL",
    "EMPTY",
    "MATCH",
    "ANY",
    "PHRASE",
    "COSINE",
    "DOT",
    "EUCLID",
    "MANHATTAN",
    "ORDER",
    "ASC",
    "DESC",
    "QUERY",
    "NEAREST",
    "CONTEXT",
    "DISCOVER",
    "PAIRS",
    "TARGET",
    "PREFETCH",
    "FUSION",
    "SAMPLE",
    "BOOST",
    "DEFAULTS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "GEO_BBOX",
    "GEO_RADIUS",
    "VALUES_COUNT",
    "HAS_VECTOR",
    "RELEVANCE",
    "FEEDBACK",
    "STAR",
    "AFTER",
    "SHARD",
    "KEY",
    "KEYS",
    "COUNT",
    "CLEAR",
    "POINTS",
    "TEXT",
    "FORMULA",
    "MMR",
    "DIVERSITY",
    "CANDIDATES",
    "RRF",
    "DBSF",
    "TRUE",
    "FALSE",
    "SIZE",
    "EMBED",
    "MULTIVECTOR",
    "NESTED",
    "QUANTIZATION",
  ].map((k) => k.toLowerCase())
)

const OPERATORS = new Set(["=", "!=", "<>", "<", ">", "<=", ">=", "*", "/", "+", "-"])

const qqlParser: StreamParser<unknown> = {
  name: "qql",
  startState() {
    return {}
  },
  token(stream) {
    if (stream.eatSpace()) return null

    // Line comment
    if (stream.match("--")) {
      stream.skipToEnd()
      return "comment"
    }

    // Block comment
    if (stream.match("/*")) {
      // eslint-disable-next-line no-empty
      while (!stream.match("*/") && !stream.eol()) {
        stream.next()
      }
      return "comment"
    }

    // String
    if (stream.match("'")) {
      while (!stream.eol()) {
        if (stream.match("''")) continue
        if (stream.next() === "'") break
      }
      return "string"
    }

    if (stream.match('"')) {
      while (!stream.eol()) {
        if (stream.match('""')) continue
        if (stream.next() === '"') break
      }
      return "string"
    }

    // Number
    if (stream.match(/^-?\d+(\.\d+)?([eE][+-]?\d+)?/)) {
      return "number"
    }

    // Punctuation / operators
    if (stream.match(/^[(),{};:[\]]/)) {
      return "punctuation"
    }

    if (stream.match(/^(!=|<>|<=|>=|=|<|>|\*|\+|\/|-)/)) {
      return "operator"
    }

    // Identifiers / keywords
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current().toLowerCase()
      if (KEYWORDS.has(word)) return "keyword"
      if (word === "true" || word === "false" || word === "null") return "atom"
      return "variableName"
    }

    stream.next()
    return null
  },
  languageData: {
    commentTokens: { line: "--", block: { open: "/*", close: "*/" } },
  },
  tokenTable: {
    keyword: t.keyword,
    string: t.string,
    number: t.number,
    comment: t.comment,
    operator: t.operator,
    punctuation: t.punctuation,
    atom: t.atom,
    variableName: t.variableName,
  },
}

// silence unused (kept for future operator styling)
void OPERATORS

export const qqlLanguage = StreamLanguage.define(qqlParser)

export const qqlKeywordsList = Array.from(KEYWORDS).map((k) => k.toUpperCase())

export const qqlCompletions = (context: { matchBefore: (reg: RegExp) => { from: number; text: string } | null; explicit: boolean; pos: number }) => {
  const word = context.matchBefore(/\w+/)
  if (!word && !context.explicit) return null

  const completions = [
    { label: "QUERY HYBRID TEXT", type: "snippet", apply: "QUERY HYBRID TEXT 'search term'\n  DENSE dense SPARSE sparse\n  FUSION RRF\n  FROM sec10k\n  LIMIT 5;", detail: "Hybrid RRF Query" },
    { label: "QUERY TEXT", type: "snippet", apply: "QUERY TEXT 'search term'\n  FROM sec10k USING dense\n  LIMIT 5;", detail: "Text vector search" },
    { label: "WITH", type: "keyword", detail: "CTE Prefetch block" },
    { label: "PREFETCH", type: "keyword", detail: "Prefetch candidates stream" },
    { label: "FUSION", type: "keyword", detail: "Fusion method (RRF/DBSF)" },
    { label: "SHARD", type: "keyword", detail: "Physical shard target" },
    { label: "WHERE", type: "keyword", detail: "Payload filter clause" },
    { label: "LIMIT", type: "keyword", detail: "Result limit" },
    { label: "SCORE THRESHOLD", type: "keyword", detail: "Score cutoff" },
    { label: "GROUP BY", type: "keyword", detail: "Aggregation" },
    { label: "FORMULA", type: "keyword", detail: "Score rewrite" },
    { label: "MMR", type: "keyword", detail: "Diversity search" },
    { label: "SCROLL FROM", type: "keyword", detail: "Pagination" },
    { label: "COUNT FROM", type: "keyword", detail: "Aggregation count" },
    { label: "UPSERT INTO", type: "keyword", detail: "Point write" },
    { label: "DELETE FROM", type: "keyword", detail: "Point delete" },
    ...qqlKeywordsList.map((k) => ({
      label: k,
      type: "keyword",
    })),
  ]

  return {
    from: word ? word.from : context.pos,
    options: completions,
  }
}
