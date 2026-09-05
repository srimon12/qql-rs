import { autocompletion, type Completion } from "@codemirror/autocomplete";
import {
	HighlightStyle,
	StreamLanguage,
	syntaxHighlighting,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { QQL_CONSTANTS, QQL_KEYWORDS } from "./qql-keywords.generated";

const keywords: Set<string> = new Set(QQL_KEYWORDS);

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
			if (QQL_CONSTANTS.has(word)) return "bool";
			return "variableName";
		}
		stream.next();
		return null;
	},
});

const completions: Completion[] = [...keywords]
	.sort()
	.map((label) => ({ label, type: "keyword" }));

export const qqlCompletion = autocompletion({
	override: [
		(context) => {
			const word = context.matchBefore(/[A-Za-z_]*/);
			if (!word || (word.from === word.to && !context.explicit)) return null;
			return {
				from: word.from,
				options: completions,
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
