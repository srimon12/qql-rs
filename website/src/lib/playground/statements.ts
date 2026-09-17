/**
 * Text-level statement mapping for the playground.
 *
 * The WASM analyzer reports how many statements a script has, but not where
 * they are. Editor affordances (statement rail, gutter markers, ⌘↵ targeting)
 * need char ranges before a run, so this scanner mirrors the lexer's trivia
 * rules — `--` line comments, `"` / `'` strings with backslash escapes and
 * SQL `''` doubling, triple-quoted strings, raw `r"…"` / `r'…'`, backticks —
 * and splits on top-level `;` only.
 */
import type { StatementSpan } from "./types";

const WHITESPACE = /\s/;

export function scanStatementSpans(source: string): StatementSpan[] {
	const spans: StatementSpan[] = [];
	const len = source.length;
	let start = 0;
	let hasContent = false;
	let depth = 0;
	let i = 0;

	while (i < len) {
		const char = source[i];

		if (WHITESPACE.test(char)) {
			i += 1;
			continue;
		}

		// `--` line comment: never carries a statement.
		if (char === "-" && source[i + 1] === "-") {
			const newline = source.indexOf("\n", i + 2);
			i = newline === -1 ? len : newline + 1;
			continue;
		}

		// Only top-level `;` separates statements — `BATCH { … }` nests them.
		if (char === ";" && depth === 0) {
			if (hasContent) spans.push({ start, end: i + 1 });
			start = i + 1;
			hasContent = false;
			i += 1;
			continue;
		}

		hasContent = true;

		if (char === "{" || char === "[" || char === "(") {
			depth += 1;
			i += 1;
			continue;
		}
		if (char === "}" || char === "]" || char === ")") {
			depth = Math.max(0, depth - 1);
			i += 1;
			continue;
		}

		// Raw strings keep their delimiter until the matching quote.
		if (char === "r" && (source[i + 1] === '"' || source[i + 1] === "'")) {
			i = skipToDelimiter(source, i + 1, source[i + 1]);
			continue;
		}
		if (char === "`") {
			i = skipToDelimiter(source, i + 1, "`");
			continue;
		}
		if (char === '"' || char === "'") {
			i = skipQuoted(source, i);
			continue;
		}

		i += 1;
	}

	if (hasContent) spans.push({ start, end: len });
	return spans;
}

/** Index just past the closing delimiter, or the end of input. */
function skipToDelimiter(
	source: string,
	from: number,
	delimiter: string,
): number {
	const found = source.indexOf(delimiter, from);
	return found === -1 ? source.length : found + delimiter.length;
}

function skipQuoted(source: string, quoteIndex: number): number {
	const quote = source[quoteIndex];
	// A triple run only opens a block string when a matching close exists.
	if (source.startsWith(quote.repeat(3), quoteIndex)) {
		const close = source.indexOf(quote.repeat(3), quoteIndex + 3);
		if (close !== -1) return close + 3;
	}
	let i = quoteIndex + 1;
	while (i < source.length) {
		const char = source[i];
		if (char === "\\") {
			i += 2;
			continue;
		}
		if (char === quote) {
			if (quote === "'" && source[i + 1] === "'") {
				i += 2;
				continue;
			}
			return i + 1;
		}
		i += 1;
	}
	return source.length;
}

/** Statement whose span contains (or most recently precedes) `pos`. */
export function statementIndexAt(spans: StatementSpan[], pos: number): number {
	if (spans.length === 0) return -1;
	for (let i = spans.length - 1; i >= 0; i -= 1) {
		if (spans[i].start <= pos) return i;
	}
	return 0;
}

/** Statements overlapping a char range (used to map a selection to results). */
export function statementIndicesInRange(
	spans: StatementSpan[],
	from: number,
	to: number,
): number[] {
	const indices: number[] = [];
	for (const [index, span] of spans.entries()) {
		if (span.end > from && span.start < to) indices.push(index);
	}
	return indices;
}

/** First keyword of a statement, upper-cased: `QUERY`, `UPSERT`, … Leading
 * comments and whitespace are trivia, not the statement. */
export function statementKeyword(source: string, span: StatementSpan): string {
	const text = source.slice(span.start, span.end);
	let i = 0;
	while (i < text.length) {
		const char = text[i];
		if (WHITESPACE.test(char)) {
			i += 1;
			continue;
		}
		if (char === "-" && text[i + 1] === "-") {
			const newline = text.indexOf("\n", i + 2);
			i = newline === -1 ? text.length : newline + 1;
			continue;
		}
		break;
	}
	const match = /[A-Za-z_][A-Za-z0-9_-]*/.exec(text.slice(i));
	return match ? match[0].toUpperCase() : "QQL";
}

/** One-line summary used in tooltips and chips. */
export function statementPreview(
	source: string,
	span: StatementSpan,
	maxLength = 88,
): string {
	const text = source
		.slice(span.start, span.end)
		.split("\n")
		.filter((line) => !line.trimStart().startsWith("--"))
		.join(" ")
		.replace(/\s+/g, " ")
		.trim();
	if (text.length <= maxLength) return text;
	return `${text.slice(0, maxLength - 1).trimEnd()}…`;
}

/** Char offset of the first non-whitespace character in a statement span. */
export function statementStartOffset(
	source: string,
	span: StatementSpan,
): number {
	const char = source.slice(span.start, span.end).search(/\S/);
	return char === -1 ? span.start : span.start + char;
}

/** Source slice for execution: surrounding whitespace removed, `;` kept. */
export function statementSource(source: string, span: StatementSpan): string {
	return source.slice(span.start, span.end).trim();
}

/** Exact visual line around `pos` — the fallback run target for broken scripts. */
export function lineAt(
	source: string,
	pos: number,
): { start: number; end: number; text: string } {
	const clamped = Math.max(0, Math.min(pos, source.length));
	const start = source.lastIndexOf("\n", clamped - 1) + 1;
	const newline = source.indexOf("\n", clamped);
	const end = newline === -1 ? source.length : newline;
	return { start, end, text: source.slice(start, end) };
}
