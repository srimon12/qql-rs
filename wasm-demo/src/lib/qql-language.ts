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
