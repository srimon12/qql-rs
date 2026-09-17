/** The single sanctioned DOM builder: no `document.createElement` elsewhere. */

type Attrs = {
	[key: string]: unknown;
	dataset?: Record<string, string>;
	text?: string | number;
	html?: string;
	onClick?: EventListener;
};

export function required<T extends Element>(selector: string): T {
	const element = document.querySelector<T>(selector);
	if (!element) throw new Error(`Playground element is missing: ${selector}`);
	return element;
}

export function all<T extends Element>(selector: string): T[] {
	return [...document.querySelectorAll<T>(selector)];
}

function setAttr(element: Element, key: string, value: string | boolean): void {
	if (key === "className" || key === "class") {
		element.setAttribute("class", String(value));
	} else if (
		key.startsWith("data") ||
		key.startsWith("aria") ||
		key === "role"
	) {
		element.setAttribute(
			key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`),
			String(value),
		);
	} else if (key in element && typeof value === "boolean") {
		(element as unknown as Record<string, unknown>)[key] = value;
	} else {
		element.setAttribute(key, String(value));
	}
}

/**
 * `h("div", { className: "…", dataset: { foo: "bar" }, title: "…" }, …children)`.
 * Children may be nodes, strings (appended as text — never parsed as HTML),
 * numbers, or null/false (skipped). Event props (`onClick`) attach listeners.
 */
export function h<K extends keyof HTMLElementTagNameMap>(
	tag: K,
	attrs?: (Attrs & { dataset?: Record<string, string> }) | null,
	...children: Array<Node | string | number | null | false | undefined>
): HTMLElementTagNameMap[K] {
	const element = document.createElement(tag);
	if (attrs) {
		for (const [key, value] of Object.entries(attrs)) {
			if (value == null || value === false) continue;
			if (key === "dataset") {
				for (const [name, entry] of Object.entries(
					value as Record<string, string>,
				)) {
					element.dataset[name] = entry;
				}
			} else if (key.startsWith("on") && typeof value === "function") {
				element.addEventListener(
					key.slice(2).toLowerCase(),
					value as EventListener,
				);
			} else if (key === "text") {
				element.textContent = String(value);
			} else if (key === "html") {
				// Escaped-safe HTML only (highlighter output). Never pass raw data.
				element.innerHTML = String(value);
			} else if (!key.startsWith("on")) {
				setAttr(element, key, value as string | boolean);
			}
		}
	}
	for (const child of children) {
		if (child == null || child === false) continue;
		element.append(
			typeof child === "string" || typeof child === "number"
				? document.createTextNode(String(child))
				: child,
		);
	}
	return element;
}

/**
 * Anchor a popover to its trigger: below, end-aligned; flips above when
 * there is no room below, clamps horizontally. Measure only while open.
 */
export function anchorPopover(menu: HTMLElement, trigger: HTMLElement): void {
	const rect = trigger.getBoundingClientRect();
	const gap = 8;
	const width = menu.offsetWidth;
	const height = menu.offsetHeight;
	let top = rect.bottom + gap;
	if (top + height > window.innerHeight - gap) {
		top = Math.max(gap, rect.top - height - gap);
	}
	let left = rect.right - width;
	const maxLeft = window.innerWidth - width - gap;
	left = Math.min(Math.max(gap, left), Math.max(gap, maxLeft));
	menu.style.top = `${Math.round(top)}px`;
	menu.style.left = `${Math.round(left)}px`;
}

/**
 * Keep a popover glued to its trigger: positions on open, re-anchors on
 * viewport resize and any scroll. One shared path for every menu.
 */
export function keepAnchored(menu: HTMLElement, trigger: HTMLElement): void {
	const place = (): void => {
		if (menu.matches(":popover-open")) anchorPopover(menu, trigger);
	};
	menu.addEventListener("toggle", place);
	window.addEventListener("resize", place);
	window.addEventListener("scroll", place, true);
}
/**
 * Clipboard copy with boolean outcome. Callers render their own confirmation;
 * pair with `.copy-btn[data-copied]` for the green `✓ Copied` treatment.
 */
export async function copyText(text: string): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		return true;
	} catch {
		return false;
	}
}

/**
 * Wire a `.copy-btn` for inline confirmation only (no toast spam): label
 * flips to `✓ Copied` + `--q-ok` for 1.6s, then reverts.
 */
export function attachCopy(
	button: HTMLButtonElement,
	getText: () => string,
): void {
	const label = button.querySelector("[data-copy-label]") ?? button;
	const original = label.textContent ?? "Copy";
	button.addEventListener("click", () => {
		void (async () => {
			const ok = await copyText(getText());
			if (!ok) return;
			button.setAttribute("data-copied", "true");
			label.textContent = "✓ Copied";
			window.setTimeout(() => {
				button.removeAttribute("data-copied");
				label.textContent = original;
			}, 1600);
		})();
	});
}
