/**
 * Qdrant connection services: the single client instance, endpoint probing,
 * embedder (re)configuration, and the live-collection presets that come from
 * a successful endpoint check.
 */

import { query } from "../core/dom";
import { refreshStatusBar } from "../core/refresh";
import { state } from "../core/store";
import { connectionHost } from "../core/wasm";
import { renderLiveCollections } from "../dialogs/presets";
import { listCollections, probeQdrant, QdrantClient } from "./client";

/** One client per page; `configureClient` reconfigures it in place. */
export const client = new QdrantClient();

/** Progress channel for the embedder status line. */
export function setEmbedStatus(message: string): void {
	const el = query("[data-embed-status]");
	if (el) el.textContent = message;
}

/**
 * Rebuild the client for the current settings. The browser embedder reports
 * its real tensor width once loaded, which updates the header pill.
 */
export function configureClient(): string {
	return client.configure(state.settings, setEmbedStatus, (dims) => {
		state.browserDims = dims;
		refreshStatusBar();
	});
}

/** Connection identity in the header pill, the settings dialog, and the footer. */
export function renderConnectionDisplay(): void {
	const host = connectionHost(state.settings.qdrantUrl);
	const connVal = query("[data-connection-value]");
	const fullEl = query("[data-connection-full]");
	const statusEndpoint = query("[data-status-endpoint]");
	if (connVal) connVal.textContent = host;
	if (statusEndpoint) statusEndpoint.textContent = host;
	if (fullEl) fullEl.textContent = state.settings.qdrantUrl;
}

/** Probe the endpoint and fill the live-collection section of the examples dialog. */
export async function checkEndpoint(): Promise<void> {
	const dot = query("[data-connected-dot]");
	const status = query("[data-status-endpoint]");
	const host = connectionHost(state.settings.qdrantUrl);
	const verdict = await probeQdrant(state.settings.qdrantUrl);
	if (dot && status) {
		dot.classList.remove("is-ready", "is-failed");
		if (verdict.kind === "ok") {
			dot.classList.add("is-ready");
			status.textContent = `${host} · online`;
		} else {
			dot.classList.add("is-failed");
			status.textContent = `${host} · unreachable`;
		}
	}

	try {
		renderLiveCollections(
			await listCollections(state.settings.qdrantUrl, state.settings.qdrantKey),
		);
	} catch {
		// live collections are a convenience; a failed fetch is not an error
	}
}
