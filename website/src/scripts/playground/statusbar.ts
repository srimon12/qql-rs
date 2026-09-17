import { all, h, required } from "./dom";
import { connectionHost } from "./errors";
import type { PlaygroundSettings } from "../playground-types";

interface StatusRefs {
	runtimeStatus: HTMLElement;
	runtimeDot: HTMLElement;
	connectionValue: HTMLElement;
	validationBadge: HTMLElement;
	statusEmbed: HTMLElement;
	policyDot: HTMLElement;
	policyChip: HTMLButtonElement;
	editorHost: HTMLElement;
	editorLoading: HTMLElement;
}

let refs: StatusRefs | null = null;

/** Bind once at boot; writers keep their call sites unchanged. */
export function initStatusBar(next: StatusRefs): void {
	refs = next;
}

function need(): StatusRefs {
	if (!refs) throw new Error("Status bar is not initialized.");
	return refs;
}

export function setRuntime(
	message: string,
	tone: "idle" | "ready" | "failed" = "idle",
): void {
	const { runtimeStatus, runtimeDot } = need();
	runtimeStatus.textContent = message;
	runtimeDot.classList.remove("is-ready", "is-failed");
	if (tone === "ready") runtimeDot.classList.add("is-ready");
	if (tone === "failed") runtimeDot.classList.add("is-failed");
}

/** Show "Ctrl" instead of "⌘" in kbd hints when the host is not Apple hardware. */
export function applyPlatformKeyHints(): void {
	const isApple = /Mac|iPhone|iPad|iPod/i.test(
		navigator.platform || navigator.userAgent,
	);
	if (isApple) return;
	for (const kbd of all<HTMLElement>("[data-kbd]")) {
		kbd.textContent = (kbd.textContent ?? "").replace("⌘", "Ctrl ");
	}
}

export function setEditorLoading(message: string | null): void {
	const { editorHost, editorLoading } = need();
	editorLoading.hidden = message == null;
	if (message != null) editorLoading.textContent = message;
	editorHost.setAttribute("aria-busy", String(message != null));
}

export function setValidationBadge(text: string): void {
	const { validationBadge } = need();
	validationBadge.replaceChildren();
	validationBadge.append(h("span", { className: "status-dot" }), text);
}

export function embedSummary(settings: PlaygroundSettings): string {
	if (settings.embedProvider === "browser") return "MiniLM (browser)";
	if (settings.embedProvider === "http") {
		return settings.embedModel
			? `HTTP ${settings.embedModel}`
			: "HTTP embedder";
	}
	return "None (vectors only)";
}

function embedBreakage(settings: PlaygroundSettings): string {
	if (settings.embedProvider === "browser") {
		return "Browser MiniLM downloads on the first TEXT query. TEXT queries fail until the download finishes. Explicit vectors always work.";
	}
	if (settings.embedProvider === "http") {
		return `TEXT queries embed through ${settings.embedUrl || "the configured endpoint"}. Explicit vectors still work when the endpoint is down.`;
	}
	return "TEXT queries fail with no embedder. Use explicit vectors such as [0.1, 0.2, 0.3].";
}

export function updateConnectionSummary(settings: PlaygroundSettings): void {
	const { connectionValue, statusEmbed } = need();
	const summary = connectionHost(settings.qdrantUrl);
	connectionValue.textContent = summary;
	// Full identity lives in the tooltip: connection chips never truncate it.
	const embedIdentity =
		settings.embedProvider === "http"
			? `${settings.embedUrl} · ${settings.embedModel || "(no model)"} · dim ${settings.embedDim}`
			: embedSummary(settings);
	const full = `Qdrant ${settings.qdrantUrl} · embeddings ${embedIdentity}. ${embedBreakage(settings)}`;
	connectionValue.parentElement?.setAttribute("title", full);
	const fullLine = document.querySelector("[data-connection-full]");
	if (fullLine) fullLine.textContent = full;
	statusEmbed.textContent =
		settings.embedProvider === "http" && settings.embedModel
			? settings.embedModel
			: embedSummary(settings);
	statusEmbed.parentElement?.setAttribute("title", full);
}

export function syncPolicyChip(policy: {
	enabled: boolean;
	field: string;
	op: string;
	value: string;
}): void {
	const { policyDot, policyChip } = need();
	policyDot.classList.toggle("is-active", policy.enabled);
	policyChip.hidden = !policy.enabled;
	const value = required<HTMLElement>("[data-policy-chip-value]");
	value.textContent = policy.enabled
		? `${policy.field} ${policy.op} ${policy.value}`
		: "";
}

/** First-run ping on the env dot: pulses until the first successful run. */
export function setFirstRunPing(on: boolean): void {
	const { runtimeDot } = need();
	runtimeDot.classList.toggle("ping", on);
}
