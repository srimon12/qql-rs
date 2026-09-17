import { all, keepAnchored, required } from "./dom";
import { toast } from "./toasts";

export function openDialog(id: string): void {
	const dialog = required<HTMLDialogElement>(id);
	dialog.showModal();
	if (id === "#preset-dialog") {
		window.setTimeout(
			() => required<HTMLInputElement>("[data-preset-search]").focus(),
			0,
		);
	}
}

export function initOverflow(): void {
	const pop = required<HTMLElement>("[data-overflow-pop]");
	const invoker = required<HTMLElement>("[data-overflow-menu]");
	keepAnchored(pop, invoker);
	pop.addEventListener("toggle", () => {
		invoker.setAttribute("aria-expanded", String(pop.matches(":popover-open")));
	});
	// The wrap checkbox-like item stays open for instant feedback; every
	// other action dismisses the menu.
	pop.addEventListener("click", (event) => {
		const target = event.target as HTMLElement;
		if (target.closest("[data-wrap-toggle]")) return;
		if (target.closest("button")) pop.hidePopover();
	});
}

export function initDialogs(): void {
	for (const dialog of all<HTMLDialogElement>("dialog[data-app-dialog]")) {
		dialog.addEventListener("click", (event) => {
			if (event.target === dialog) dialog.close();
		});
		for (const close of all<HTMLButtonElement>(
			`#${dialog.id} [data-close-dialog]`,
		)) {
			close.addEventListener("click", () => dialog.close());
		}
	}
	for (const button of all<HTMLElement>("[data-open-presets]")) {
		button.addEventListener("click", () => openDialog("#preset-dialog"));
	}
	for (const button of all<HTMLElement>("[data-open-settings]")) {
		button.addEventListener("click", () => openDialog("#settings-dialog"));
	}
	required("[data-open-policy]").addEventListener("click", () =>
		openDialog("#policy-dialog"),
	);
	required("[data-policy-chip]").addEventListener("click", () =>
		openDialog("#policy-dialog"),
	);
}
function currentShareUrl(getSource: () => string): URL {
	const docsBacklink = required<HTMLAnchorElement>("[data-docs-backlink]");
	const url = new URL(window.location.href);
	url.search = "";
	url.hash = "";
	url.searchParams.set("q", getSource());
	const ref = docsBacklink.hidden ? null : docsBacklink.getAttribute("href");
	if (ref && isSafeDocsRef(ref)) url.searchParams.set("ref", ref);
	return url;
}

function shareHeadline(source: string): string {
	const line =
		source
			.split("\n")
			.map((row) => row.trim())
			.find((row) => row && !row.startsWith("--")) ?? "a QQL query";
	const clipped = line.length > 72 ? `${line.slice(0, 69)}…` : line;
	return `Try this in the QQL playground:\n${clipped}`;
}

function openShareMenu(): void {
	required<HTMLElement>("[data-overflow-pop]").hidePopover();
	const menu = required<HTMLElement>("[data-share-menu]");
	if (!("showPopover" in HTMLElement.prototype)) {
		menu.classList.toggle("app-share-pop--open");
		return;
	}
	menu.showPopover();
}

/** Share popover; source text comes from the editor via getSource. */
export function initShare(getSource: () => string): void {
	const menu = required<HTMLElement>("[data-share-menu]");
	keepAnchored(menu, required<HTMLElement>("[data-overflow-menu]"));
	const urlInput = required<HTMLInputElement>("[data-share-url]");
	const copyButton = required<HTMLButtonElement>("[data-share-copy]");
	const copyLabel = required<HTMLElement>("[data-share-copy-label]");
	const xLink = required<HTMLAnchorElement>("[data-share-x]");
	const linkedinLink = required<HTMLAnchorElement>("[data-share-linkedin]");
	const nativeButton = document.querySelector<HTMLButtonElement>(
		"[data-share-native]",
	);
	const canPopover = "showPopover" in HTMLElement.prototype;
	let copiedTimer = 0;

	const fill = (): string => {
		const url = currentShareUrl(getSource).toString();
		const text = shareHeadline(getSource());
		urlInput.value = url;
		xLink.href = `https://x.com/intent/tweet?text=${encodeURIComponent(text)}&url=${encodeURIComponent(url)}`;
		linkedinLink.href = `https://www.linkedin.com/sharing/share-offsite/?url=${encodeURIComponent(url)}`;
		return url;
	};

	const markCopied = (copied: boolean): void => {
		if (copied) copyButton.dataset.copied = "true";
		else delete copyButton.dataset.copied;
		copyLabel.textContent = copied ? "Copied" : "Copy";
		copyButton.setAttribute("aria-label", copied ? "Link copied" : "Copy link");
	};

	const copyShareUrl = async (): Promise<void> => {
		const url = fill();
		try {
			await navigator.clipboard.writeText(url);
			markCopied(true);
			toast("Share link copied.");
			window.clearTimeout(copiedTimer);
			copiedTimer = window.setTimeout(() => markCopied(false), 2000);
		} catch {
			urlInput.select();
			toast(
				"Clipboard access was denied. Copy the URL from the field.",
				"error",
			);
		}
	};

	if (nativeButton && "share" in navigator) {
		nativeButton.hidden = false;
		nativeButton.addEventListener("click", async () => {
			const url = fill();
			try {
				await navigator.share({
					title: "QQL Playground",
					text: shareHeadline(getSource()),
					url,
				});
			} catch (error) {
				if (error instanceof Error && error.name === "AbortError") return;
				toast("Share failed.", "error");
			}
		});
	}

	if (!canPopover) {
		required("[data-overflow-share]").addEventListener(
			"click",
			() => void copyShareUrl(),
		);
		return;
	}

	required("[data-overflow-share]").addEventListener("click", openShareMenu);

	menu.addEventListener("toggle", () => {
		const open = menu.matches(":popover-open");
		if (!open) {
			markCopied(false);
			return;
		}
		fill();
		urlInput.focus();
		urlInput.select();
	});

	copyButton.addEventListener("click", () => void copyShareUrl());
	urlInput.addEventListener("focus", () => urlInput.select());
}

export function initDocsBacklink(ref: string | null): void {
	const docsBacklink = required<HTMLAnchorElement>("[data-docs-backlink]");
	if (!ref || !isSafeDocsRef(ref)) return;
	// Rebuild the link from the parsed, same-origin URL instead of the raw
	// parameter; encodeURI is the final barrier so no scheme, authority, or
	// markup can reach the DOM. For same-origin /docs paths this is
	// byte-identical to `ref` (encodeURI preserves / ? & = : #).
	const target = new URL(ref, window.location.origin);
	docsBacklink.setAttribute("href", encodeURI(target.pathname + target.search));
	docsBacklink.hidden = false;
}
function isSafeDocsRef(ref: string): boolean {
	// Allow only same-origin /docs/* paths. Reject backslashes, control
	// chars, and HTML-significant chars so the value cannot break out of
	// the href context, and re-parse to block encoded // or scheme tricks.
	if (!/^\/docs(?:\/|$)/.test(ref)) return false;
	if (/[\\<>"'`\s]/.test(ref)) return false;
	try {
		const parsed = new URL(ref, window.location.origin);
		if (parsed.origin !== window.location.origin) return false;
		if (
			!(parsed.pathname === "/docs" || parsed.pathname.startsWith("/docs/"))
		) {
			return false;
		}
		return true;
	} catch {
		return false;
	}
}
