import { probeEmbedder as probeEmbedderIO, probeQdrant } from "./client";
import { required } from "./dom";
import { connectionHost } from "./errors";
import { store } from "./store";
import { toast } from "./toasts";

const statusEndpoint = required<HTMLElement>("[data-status-endpoint]");
const connectedDot = required<HTMLElement>("[data-connected-dot]");
const embedProbe = required<HTMLElement>("[data-embed-probe]");
const embedDimChip = required<HTMLElement>("[data-embed-dim-chip]");

let onDown: (() => void) | null = null;

/** Bind once at boot; the down-branch re-renders through this callback. */
export function initHealth(hooks: { renderOutputs: () => void }): void {
	onDown = hooks.renderOutputs;
}

export async function checkEndpoint(manual = false): Promise<void> {
	const base = store.settings.qdrantUrl.replace(/\/+$/, "");
	statusEndpoint.textContent = "Checking";
	connectedDot.classList.remove("is-ready", "is-failed");

	let qdrantOk = false;
	let qdrantLine: string;
	const verdict = await probeQdrant(base);
	if (verdict.kind === "http-error") {
		statusEndpoint.textContent = `Reachable, HTTP ${verdict.status}`;
		connectedDot.classList.add("is-ready");
		qdrantLine = `Qdrant answered but refused the request (HTTP ${verdict.status}). Set the API key in Connection settings when the instance requires one. Offline analyze still works.`;
		store.noteQdrant(true);
	} else if (verdict.kind === "ok") {
		statusEndpoint.textContent = `Reachable (${connectionHost(base)})`;
		connectedDot.classList.add("is-ready");
		qdrantLine = "Qdrant is reachable.";
		qdrantOk = true;
		store.noteQdrant(true);
	} else {
		if (verdict.kind === "cors") {
			statusEndpoint.textContent = "Blocked by browser CORS policy";
			qdrantLine = `Your Qdrant answered at ${base}, but the browser blocked the response. Allow this page origin in the Qdrant CORS settings, then reload. Offline analyze still works.`;
		} else {
			statusEndpoint.textContent = `Not reachable (${connectionHost(base)})`;
			qdrantLine = `Qdrant is not reachable at ${base}. Start Qdrant locally with the default REST port 6333 and reload. Offline analyze still works without it.`;
		}
		connectedDot.classList.add("is-failed");
		store.noteQdrant(verdict.kind !== "down");
		if (store.qdrant === "down") onDown?.();
	}

	// The embedder probe is independent: an embedder outage never fails the
	// Qdrant verdict above, and each side is reported on its own channel.
	const embedder = await probeEmbedder();
	if (manual) {
		toast(
			`${qdrantLine} · ${embedder.line}`,
			qdrantOk && embedder.ok ? "success" : "error",
		);
	}
}

/**
 * POST-probe the configured HTTP embedder (model + dimension check).
 * Reports only through the embedder channels; Qdrant status is untouched.
 */
async function probeEmbedder(): Promise<{ ok: boolean; line: string }> {
	embedDimChip.hidden = true;
	embedProbe.textContent = `Probing ${store.settings.embedUrl}…`;
	const verdict = await probeEmbedderIO(store.settings);
	if (verdict.probe === "browser") {
		embedProbe.textContent =
			"Browser MiniLM loads on the first TEXT query — no endpoint to probe.";
		return { ok: true, line: "browser embedder (loads on first TEXT query)" };
	}
	if (verdict.probe === "none") {
		embedProbe.textContent = "Embeddings disabled — use explicit vectors.";
		return { ok: true, line: "no embedder (vectors only)" };
	}
	if (verdict.dim == null) {
		const line =
			"httpStatus" in verdict && verdict.httpStatus != null
				? `Embedder HTTP ${verdict.httpStatus} at ${connectionHost(store.settings.embedUrl)} — TEXT queries fail, vectors still work.`
				: "unreachable" in verdict
					? `Embedder unreachable at ${connectionHost(store.settings.embedUrl)} — TEXT queries fail, vectors still work.`
					: "Embedder answered but returned no vector — TEXT queries will fail.";
		embedProbe.textContent = line;
		return { ok: false, line };
	}
	const dim = verdict.dim;
	if (dim === store.settings.embedDim) {
		embedDimChip.hidden = false;
		embedDimChip.textContent = `dim ${dim} ✓`;
		embedDimChip.title = `${store.settings.embedModel} returned dim ${dim}, as configured.`;
		embedProbe.textContent = `Embedder OK — ${store.settings.embedModel}, dim ${dim} ✓.`;
		return { ok: true, line: `embedder dim ${dim} ✓` };
	}
	embedDimChip.hidden = false;
	embedDimChip.textContent = `dim ${dim} ≠ ${store.settings.embedDim}`;
	embedDimChip.title = `Endpoint returned dim ${dim} but Dimension is ${store.settings.embedDim}.`;
	const line = `Embedder dim ${dim} ≠ configured ${store.settings.embedDim} — set Dimension to ${dim}.`;
	embedProbe.textContent = line;
	return { ok: false, line };
}

/** Draggable divider between the editor and inspector panels (lg and up). */
