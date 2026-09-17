export function pretty(value: unknown): string {
	// u64/i64 (point IDs, counts, integer payloads) cross the WASM boundary
	// as BigInt, which JSON.stringify rejects. Safe-range values render as
	// the plain digits they always were (they paste straight back into QQL);
	// larger ones keep the `n` literal suffix so snowflake IDs stay exact and
	// visibly numeric instead of rounding into a Number.
	return JSON.stringify(
		value,
		(_key, nested: unknown) =>
			typeof nested === "bigint"
				? printableBigInt(nested)
				: (nested as unknown),
		2,
	);
}

export function printableBigInt(value: bigint): string {
	const digits = value.toString();
	const safe =
		value <= BigInt(Number.MAX_SAFE_INTEGER) &&
		value >= BigInt(Number.MIN_SAFE_INTEGER);
	return safe ? digits : `${digits}n`;
}

export function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;");
}

/** Display-only JSON: unquote BigInt-suffixed strings so snowflakes read numeric. */
export function displayJson(value: unknown): string {
	return pretty(value).replace(/"(-?\d+n)"/g, "$1");
}

/** Pretty-print a raw JSON string; fall back to the raw text when unparseable. */
export function prettyRaw(raw: string): string {
	try {
		return JSON.stringify(JSON.parse(raw), null, 2);
	} catch {
		return raw;
	}
}

const JSON_TOKEN_RE =
	/("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?n?)|\b(true|false|null)\b/g;

/** Tiny brand-palette highlighter for JSON dumps (keys/strings/numbers). */
export function highlightJson(source: string): string {
	let result = "";
	let last = 0;
	source.replace(
		JSON_TOKEN_RE,
		(
			match: string,
			str: string,
			colon: string,
			num: string,
			lit: string,
			offset: number,
		) => {
			result += escapeHtml(source.slice(last, offset));
			if (str !== undefined) {
				const inner = str.slice(1, -1);
				const cls =
					colon !== undefined
						? "tok-key"
						: /^-?\d+n$/.test(inner)
							? "tok-num"
							: "tok-str";
				result += `<span class="${cls}">${escapeHtml(str)}</span>${colon === undefined ? "" : escapeHtml(colon)}`;
			} else if (num !== undefined) {
				result += `<span class="tok-num">${escapeHtml(num)}</span>`;
			} else {
				result += `<span class="tok-lit">${escapeHtml(lit)}</span>`;
			}
			last = offset + match.length;
			return match;
		},
	);
	return result + escapeHtml(source.slice(last));
}

const EXPLAIN_TOKEN_RE =
	/(QQL-[A-Z0-9-]+)|("(?:[^"\\]|\\.)*")|(-?\d+(?:\.\d+)?)/g;

/** Same palette for the human-readable plan: codes, quoted text, numbers. */
export function highlightExplain(source: string): string {
	let result = "";
	let last = 0;
	source.replace(
		EXPLAIN_TOKEN_RE,
		(match: string, code: string, str: string, num: string, offset: number) => {
			result += escapeHtml(source.slice(last, offset));
			if (code !== undefined) {
				result += `<span class="tok-key">${escapeHtml(code)}</span>`;
			} else if (str !== undefined) {
				result += `<span class="tok-str">${escapeHtml(str)}</span>`;
			} else {
				result += `<span class="tok-num">${escapeHtml(num)}</span>`;
			}
			last = offset + match.length;
			return match;
		},
	);
	return result + escapeHtml(source.slice(last));
}

const LONG_ARRAY_RE = /\[([\s\d,.\-+eE]+)\]/g;

/**
 * Collapse 12+ number runs (`[0.1, 0.2, …]`) into `[N dims — show]`
 * disclosures so embedding arrays don't blow up the Wire tab.
 */
export function highlightJsonCollapsed(source: string): string {
	let html = "";
	let last = 0;
	LONG_ARRAY_RE.lastIndex = 0;
	for (;;) {
		const match = LONG_ARRAY_RE.exec(source);
		if (!match || match.index == null) break;
		const nums = match[1]
			.split(",")
			.map((part) => part.trim())
			.filter(Boolean);
		if (nums.length < 12 || !nums.every((num) => /^-?\d/.test(num))) continue;
		html += highlightJson(source.slice(last, match.index));
		html += `<details class="vec-collapse"><summary class="vec-summary">[${nums.length} dims — show]</summary>${highlightJson(match[0])}</details>`;
		last = match.index + match[0].length;
	}
	return html + highlightJson(source.slice(last));
}

/** Render a JSON tab with highlighting; plain-text tabs keep raw text. */
export function renderHighlighted(pre: HTMLElement, value: unknown): void {
	pre.innerHTML =
		typeof value === "string"
			? escapeHtml(value)
			: highlightJsonCollapsed(displayJson(value));
}

export function renderExplain(pre: HTMLElement, value: unknown): void {
	pre.innerHTML =
		typeof value === "string"
			? highlightExplain(value)
			: highlightJson(displayJson(value));
}

/** Snowflake-safe id text: safe-range plain digits, larger `…n`-suffixed. */
export function formatId(id: unknown): string {
	return typeof id === "bigint" ? printableBigInt(id) : String(id);
}

export function formatScalar(value: unknown): string {
	if (typeof value === "bigint") return printableBigInt(value);
	if (typeof value === "string") return value;
	if (value == null) return "null";
	if (typeof value === "number" || typeof value === "boolean")
		return String(value);
	const flat = JSON.stringify(value);
	return flat.length > 160 ? `${flat.slice(0, 157)}…` : flat;
}
