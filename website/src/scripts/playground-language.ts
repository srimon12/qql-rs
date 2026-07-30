import { autocompletion, type Completion } from "@codemirror/autocomplete";
import {
  HighlightStyle,
  StreamLanguage,
  syntaxHighlighting,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";

const keywords = new Set(
  [
    "ALL",
    "AND",
    "AS",
    "ASC",
    "AT",
    "BY",
    "CLEAR",
    "CLUSTER",
    "COLLECTION",
    "CREATE",
    "DELETE",
    "DESC",
    "DISTANCE",
    "DROP",
    "EXISTS",
    "FROM",
    "FULL",
    "INDEX",
    "INTO",
    "LIMIT",
    "MAX",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "PAYLOAD",
    "QUERY",
    "REPLICAS",
    "SELECT",
    "SET",
    "SHARD",
    "SHARDS",
    "TEXT",
    "TOP",
    "UNSET",
    "UPDATE",
    "UPSERT",
    "VECTOR",
    "VECTORS",
    "WHERE",
    "WITH",
  ].map((word) => word.toUpperCase()),
);

const values = new Set(["TRUE", "FALSE", "NULL"]);

export const qqlLanguage = StreamLanguage.define({
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match("--")) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.match(/^(?:r#*"|["'])/)) {
      const quote = stream.current().at(-1) ?? '"';
      let escaped = false;
      while (!stream.eol()) {
        const char = stream.next();
        if (char === quote && !escaped) break;
        escaped = char === "\\" && !escaped;
        if (char !== "\\") escaped = false;
      }
      return "string";
    }
    if (stream.match(/^-?(?:\d+\.\d+|\d+)/)) return "number";
    if (stream.match(/^[()[\]{},.;:+*/<>=-]/)) return "operator";
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_-]*/)) {
      const word = stream.current().toUpperCase();
      if (keywords.has(word)) return "keyword";
      if (values.has(word)) return "bool";
      return "variableName";
    }
    stream.next();
    return null;
  },
});

const completions: Completion[] = [...keywords]
  .sort()
  .map((label) => ({ label, type: "keyword" }));

const snippets: Completion[] = [
  {
    label: "QUERY hybrid",
    type: "text",
    detail: "Hybrid vector and text query",
    apply:
      "QUERY collection_name\n  NEAR [0.1, 0.2, 0.3]\n  WHERE active = true\n  TOP 10;",
  },
  {
    label: "SELECT",
    type: "text",
    detail: "Select points",
    apply: "SELECT * FROM collection_name\nWHERE active = true\nLIMIT 10;",
  },
  {
    label: "UPSERT",
    type: "text",
    detail: "Upsert a point",
    apply:
      'UPSERT INTO collection_name\nVALUES ({ id: 1, vector: [0.1, 0.2], payload: { title: "Example" } });',
  },
];

export const qqlCompletion = autocompletion({
  override: [
    (context) => {
      const word = context.matchBefore(/[A-Za-z_]*/);
      if (!word || (word.from === word.to && !context.explicit)) return null;
      return {
        from: word.from,
        options: [...snippets, ...completions],
        validFor: /^[A-Za-z_]*$/,
      };
    },
  ],
});

export const qqlHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.keyword, color: "var(--syntax-keyword)", fontWeight: "650" },
    { tag: tags.string, color: "var(--syntax-string)" },
    { tag: [tags.number, tags.bool], color: "var(--syntax-number)" },
    { tag: tags.comment, color: "var(--syntax-comment)", fontStyle: "italic" },
    { tag: tags.operator, color: "var(--syntax-operator)" },
    { tag: tags.variableName, color: "var(--syntax-name)" },
  ]),
);
