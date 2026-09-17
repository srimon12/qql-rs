/**
 * Shared DOM plumbing for every playground surface: element lookup, HTML
 * escaping, toasts, clipboard writes, dialog wiring, and platform key labels.
 * Everything here is presentation-agnostic; no playground state is read.
 */

export function byId<T extends HTMLElement>(id: string): T {
	const el = document.getElementById(id);
	if (!el) throw new Error(`Missing element #${id}`);
	return el as T;
}

export function query<T extends HTMLElement>(
	selector: string,
	root: ParentNode = document,
): T | null {
	return root.querySelector<T>(selector);
}

export function queryAll<T extends HTMLElement>(
	selector: string,
	root: ParentNode = document,
): T[] {
	return [...root.querySelectorAll<T>(selector)];
}

export function escapeHtml(value: unknown): string {
	if (value == null) return "";
	return String(value)
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#039;");
}

// ── Platform ─────────────────────────────────────────────────────────────────

let macPlatform: boolean | null = null;

/** macOS-style `⌘` vs `Ctrl` hints; memoized — the UA never changes. */
export function isMacPlatform(): boolean {
	if (macPlatform === null) {
		macPlatform = /Mac|iPhone|iPad|iPod/.test(
			`${navigator.platform} ${navigator.userAgent}`,
		);
	}
	return macPlatform;
}

/** Rewrite the static kbd hints declared in markup for the current platform. */
export function applyPlatformKeyLabels(): void {
	const mac = isMacPlatform();
	queryAll("[data-kbd], [data-kbd-short]").forEach((kbd) => {
		kbd.textContent = mac ? "⌘↵" : "Ctrl+↵";
	});
	queryAll("[data-kbd-all]").forEach((kbd) => {
		kbd.textContent = mac ? "⇧⌘↵" : "Shift+Ctrl+↵";
	});
	queryAll("[data-open-palette] kbd").forEach((kbd) => {
		kbd.textContent = mac ? "⌘K" : "Ctrl+K";
	});
}

// ── Feedback ─────────────────────────────────────────────────────────────────

export function showToast(message: string, isError = false): void {
	const region = query("[data-toast-region]");
	if (!region) return;
	const toast = document.createElement("div");
	toast.className = `toast ${isError ? "toast--error" : ""}`;
	toast.textContent = message;
	region.appendChild(toast);
	window.setTimeout(() => toast.remove(), 3200);
}

/** Clipboard write + toast. Returns false when the browser denies access. */
export async function copyToClipboard(
	text: string,
	successMessage: string,
): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		showToast(successMessage);
		return true;
	} catch {
		showToast("Clipboard unavailable — copy manually instead", true);
		return false;
	}
}

// ── Dialogs ──────────────────────────────────────────────────────────────────

export function openDialog(id: string): void {
	byId<HTMLDialogElement>(id).showModal();
}

/**
 * Wire a native `<dialog>`: openers, `[data-close-dialog]` buttons, and
 * backdrop clicks all close it.
 */
export function setupDialog(
	dialogId: string,
	openSelectors: string[],
): HTMLDialogElement {
	const dialog = byId<HTMLDialogElement>(dialogId);
	for (const selector of openSelectors) {
		queryAll(selector).forEach((trigger) => {
			trigger.addEventListener("click", () => dialog.showModal());
		});
	}
	queryAll("[data-close-dialog]", dialog).forEach((close) => {
		close.addEventListener("click", () => dialog.close());
	});
	dialog.addEventListener("click", (event) => {
		if (event.target === dialog) dialog.close();
	});
	return dialog;
}
