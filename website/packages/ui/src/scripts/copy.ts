/**
 * The site's only copy implementation.
 *
 * One behavior for every copy surface: docs code frames, QQL examples, the
 * docs "Copy page" action, the landing hero command, install commands, and
 * the comparator. Buttons opt in with `data-copy` (literal text),
 * `data-copy-target` (selector), `data-copy-source="article"` (docs page as
 * Markdown), or by living inside an `.expressive-code` frame (closest code).
 *
 * No dependencies: the clipboard API with an execCommand fallback, and a
 * small DOM-to-Markdown converter for the docs page action.
 */

const COPIED_ATTR = "data-copied";
const FAILED_ATTR = "data-copied-failed";
const FEEDBACK_MS = 1600;
const SKIP_TAGS = new Set([
	"SCRIPT",
	"STYLE",
	"BUTTON",
	"NAV",
	"SVG",
	"NOSCRIPT",
]);
const HEADING_TAGS: Record<string, number> = {
	H1: 1,
	H2: 2,
	H3: 3,
	H4: 4,
	H5: 5,
	H6: 6,
};

/** Clipboard write with a legacy fallback for non-secure or restricted contexts. */
export async function writeClipboard(text: string): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		return true;
	} catch {
		return legacyWrite(text);
	}
}

function legacyWrite(text: string): boolean {
	const scratch = document.createElement("pre");
	Object.assign(scratch.style, {
		position: "fixed",
		top: "0",
		left: "0",
		opacity: "0",
		pointerEvents: "none",
	});
	scratch.setAttribute("aria-hidden", "true");
	scratch.textContent = text;
	document.body.append(scratch);
	const selection = window.getSelection();
	const range = document.createRange();
	range.selectNodeContents(scratch);
	let ok = false;
	if (selection) {
		selection.removeAllRanges();
		selection.addRange(range);
		try {
			ok = document.execCommand("copy");
		} finally {
			selection.removeAllRanges();
		}
	}
	scratch.remove();
	return ok;
}

/**
 * Text extraction that understands Expressive Code's DOM: lines are
 * `.ec-line` divs with no text newlines, so join them explicitly. Anything
 * else falls back to the nearest `<pre>` or the element's own text.
 */
export function extractText(el: HTMLElement): string {
	const lines = el.querySelectorAll<HTMLElement>(".ec-line");
	if (lines.length > 0) {
		return [...lines]
			.map((line) => line.textContent ?? "")
			.join("\n")
			.trimEnd();
	}
	const pre = el.matches("pre") ? el : el.querySelector("pre");
	return ((pre ?? el).textContent ?? "").trimEnd();
}

export function resolveCopyText(btn: HTMLElement): string | null {
	if (btn.dataset.copy !== undefined) return btn.dataset.copy;
	const selector = btn.dataset.copyTarget;
	if (selector) {
		const target = document.querySelector<HTMLElement>(selector);
		return target ? extractText(target) : null;
	}
	if (btn.dataset.copySource === "article") {
		const article = document.querySelector<HTMLElement>(
			"main .sl-markdown-content",
		);
		if (!article) return null;
		const title = document
			.querySelector<HTMLElement>("main h1#_top")
			?.textContent?.trim();
		const body = pageToMarkdown(article);
		return title ? `# ${title}\n\n${body}` : body;
	}
	const frame = btn.closest<HTMLElement>(".expressive-code");
	if (frame) return extractText(frame);
	const pre = btn.closest("pre");
	return pre ? extractText(pre) : null;
}

function flash(btn: HTMLElement, ok: boolean): void {
	const label = btn.querySelector<HTMLElement>("[data-copy-label]");
	const status = btn.querySelector<HTMLElement>("[data-copy-status]");
	btn.toggleAttribute(COPIED_ATTR, ok);
	btn.toggleAttribute(FAILED_ATTR, !ok);
	if (label) {
		label.textContent = ok
			? (btn.dataset.copiedLabel ?? "Copied")
			: (btn.dataset.failedLabel ?? "Failed");
	}
	if (status) status.textContent = ok ? "Copied to clipboard" : "Copy failed";
	window.setTimeout(() => {
		btn.removeAttribute(COPIED_ATTR);
		btn.removeAttribute(FAILED_ATTR);
		if (label) label.textContent = btn.dataset.copyLabel ?? "Copy";
		if (status) status.textContent = "";
	}, FEEDBACK_MS);
}

async function handleCopy(btn: HTMLElement): Promise<void> {
	const text = resolveCopyText(btn);
	if (text == null || text === "") return;
	flash(btn, await writeClipboard(text));
}

/** Wire every copy button under `root` (idempotent). */
export function mountCopyButtons(root: ParentNode = document): void {
	for (const btn of root.querySelectorAll<HTMLElement>(
		"[data-copy], [data-copy-target], [data-copy-source]",
	)) {
		bind(btn);
	}
}

function bind(btn: HTMLElement): void {
	if (btn.dataset.copyBound === "true") return;
	btn.dataset.copyBound = "true";
	btn.addEventListener("click", () => {
		void handleCopy(btn);
	});
}

function iconSvg(kind: "copy" | "check"): string {
	const body =
		kind === "copy"
			? '<rect width="14" height="14" x="8" y="8" rx="2"></rect><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"></path>'
			: '<path d="M20 6 9 17l-5-5"></path>';
	return `<svg class="copy-btn__icon copy-btn__icon--${kind}" viewBox="0 0 24 24" aria-hidden="true">${body}</svg>`;
}

/** Build a copy button matching CopyButton.astro's markup (for injected surfaces). */
export function createCopyButton(options: {
	variant?: "link" | "icon" | "page";
	label?: string;
	copiedLabel?: string;
	extraClass?: string;
	title?: string;
}): HTMLButtonElement {
	const { variant = "link", label = "Copy", copiedLabel = "Copied" } = options;
	const btn = document.createElement("button");
	btn.type = "button";
	btn.className = ["copy-btn", `copy-btn--${variant}`, options.extraClass]
		.filter(Boolean)
		.join(" ");
	btn.dataset.copyLabel = label;
	btn.dataset.copiedLabel = copiedLabel;
	if (options.title) btn.title = options.title;
	// `iconSvg` bodies are static literals; the caller-supplied `label` must
	// never flow through `innerHTML` (CodeQL js/xss). Build label/status via
	// textContent so library input cannot inject markup.
	btn.innerHTML = `${iconSvg("copy")}${iconSvg("check")}`;
	const labelSpan = document.createElement("span");
	labelSpan.className = "copy-btn__label";
	labelSpan.setAttribute("data-copy-label", "");
	labelSpan.textContent = label;
	btn.append(labelSpan);
	const statusSpan = document.createElement("span");
	statusSpan.className = "sr-only";
	statusSpan.setAttribute("data-copy-status", "");
	statusSpan.setAttribute("aria-live", "polite");
	btn.append(statusSpan);
	return btn;
}

/** Docs pages: page-title action + one button per code frame (replaces EC's). */
export function mountDocsCopy(): void {
	mountCopyButtons();
	mountPageCopyButton();
	mountCodeFrameButtons();
}

function mountCodeFrameButtons(): void {
	for (const frame of document.querySelectorAll<HTMLElement>(
		".expressive-code .frame",
	)) {
		if (frame.querySelector(".copy-btn")) continue;
		if (frame.closest("[data-copy-managed]")) continue;
		const btn = createCopyButton({
			variant: "icon",
			extraClass: "copy-btn--frame",
			title: "Copy code",
		});
		btn.setAttribute("aria-label", "Copy code");
		bind(btn);
		frame.append(btn);
	}
}

function mountPageCopyButton(): void {
	const title = document.querySelector<HTMLElement>("main h1#_top");
	if (!title || document.querySelector("[data-copy-page]")) return;
	const row = document.createElement("div");
	row.className = "docs-title-row";
	title.before(row);
	row.append(title);
	const btn = createCopyButton({
		variant: "page",
		label: "Copy page",
		copiedLabel: "Copied",
	});
	btn.setAttribute("data-copy-page", "true");
	btn.setAttribute("data-copy-source", "article");
	row.append(btn);
	mountCopyButtons(row);
}

/* ── Docs page → Markdown (small, hand-rolled, covers our content) ──────── */

function inlineText(node: Node): string {
	if (node.nodeType === Node.TEXT_NODE) {
		return (node.textContent ?? "").replace(/\s+/g, " ");
	}
	if (!(node instanceof HTMLElement)) return "";
	if (SKIP_TAGS.has(node.tagName) || node.classList.contains("sl-anchor-link"))
		return "";
	const children = [...node.childNodes].map(inlineText).join("");
	switch (node.tagName) {
		case "CODE": {
			const code = (node.textContent ?? "").trim();
			return code ? `\`${code}\`` : "";
		}
		case "STRONG":
		case "B":
			return `**${children.trim()}**`;
		case "EM":
		case "I":
			return `*${children.trim()}*`;
		case "A": {
			const text = children.trim();
			const href = node.getAttribute("href") ?? "";
			if (!text || !href || href.startsWith("#")) return text;
			return `[${text}](${href})`;
		}
		case "IMG": {
			const src = node.getAttribute("src") ?? "";
			return src ? `![${node.getAttribute("alt") ?? ""}](${src})` : "";
		}
		default:
			return children;
	}
}

function codeFence(pre: HTMLElement): string {
	const lang =
		pre.closest<HTMLElement>("[data-language]")?.dataset.language ??
		pre.querySelector("code")?.className.match(/language-([\w-]+)/)?.[1] ??
		"";
	const code = extractText(pre);
	return `\`\`\`${lang}\n${code}\n\`\`\``;
}

function escapeTableCell(text: string): string {
	// Escape backslashes first so `\|` round-trips, then pipes, then fold
	// newlines (CodeQL js/incomplete-sanitization: backslash must be escaped).
	return text
		.trim()
		.replace(/\\/g, "\\\\")
		.replace(/\|/g, "\\|")
		.replace(/\r?\n/g, " ");
}

function tableMarkdown(table: HTMLElement, out: string[]): void {
	const rows = [...table.querySelectorAll("tr")].map((tr) =>
		[...tr.children].map((cell) => escapeTableCell(inlineText(cell))),
	);
	if (rows.length === 0) return;
	out.push(`| ${rows[0].join(" | ")} |`);
	out.push(`| ${rows[0].map(() => "---").join(" | ")} |`);
	for (const row of rows.slice(1)) out.push(`| ${row.join(" | ")} |`);
}

function blockMarkdown(node: Node, out: string[], depth = 0): void {
	if (node.nodeType === Node.TEXT_NODE) {
		const text = (node.textContent ?? "").trim();
		if (text) out.push(text);
		return;
	}
	if (!(node instanceof HTMLElement)) return;
	const tag = node.tagName;
	if (SKIP_TAGS.has(tag) || node.classList.contains("sl-anchor-link")) return;
	if (HEADING_TAGS[tag]) {
		const text = inlineText(node).trim();
		if (text) out.push(`${"#".repeat(HEADING_TAGS[tag])} ${text}`);
		return;
	}
	if (tag === "P") {
		const text = inlineText(node).trim();
		if (text) out.push(text);
		return;
	}
	if (tag === "PRE") {
		out.push(codeFence(node));
		return;
	}
	if (tag === "UL" || tag === "OL") {
		const items: string[] = [];
		let index = 1;
		for (const li of node.children) {
			if (!(li instanceof HTMLElement)) continue;
			const inlineParts: ChildNode[] = [];
			const nested: Node[] = [];
			for (const child of li.childNodes) {
				if (
					child instanceof HTMLElement &&
					(child.tagName === "UL" || child.tagName === "OL")
				) {
					nested.push(child);
				} else {
					inlineParts.push(child);
				}
			}
			const text = inlineParts
				.map((node) => inlineText(node))
				.join("")
				.trim();
			const marker = tag === "OL" ? `${index++}.` : "-";
			if (text) items.push(`${"  ".repeat(depth)}${marker} ${text}`);
			for (const child of nested) blockMarkdown(child, items, depth + 1);
		}
		if (items.length > 0) out.push(items.join("\n"));
		return;
	}
	if (tag === "TABLE") {
		tableMarkdown(node, out);
		return;
	}
	if (tag === "BLOCKQUOTE") {
		const inner: string[] = [];
		for (const child of node.childNodes) blockMarkdown(child, inner);
		if (inner.length > 0) {
			out.push(
				inner
					.join("\n\n")
					.split("\n")
					.map((line) => `> ${line}`)
					.join("\n"),
			);
		}
		return;
	}
	if (tag === "DETAILS") {
		const summary = node.querySelector("summary");
		const rest: Node[] = [...node.childNodes].filter(
			(child) => child !== summary,
		);
		const restBlocks: string[] = [];
		for (const child of rest) blockMarkdown(child, restBlocks);
		const summaryText = inlineText(summary ?? node).trim();
		const combined = [summaryText ? `**${summaryText}**` : "", ...restBlocks]
			.filter(Boolean)
			.join("\n\n");
		if (combined) out.push(combined);
		return;
	}
	if (tag === "HR") {
		out.push("---");
		return;
	}
	for (const child of node.childNodes) blockMarkdown(child, out, depth);
}

/** Convert a Starlight article body to Markdown for the "Copy page" action. */
export function pageToMarkdown(root: HTMLElement): string {
	const out: string[] = [];
	blockMarkdown(root, out);
	return `${out
		.join("\n\n")
		.replace(/\n{3,}/g, "\n\n")
		.trim()}\n`;
}
