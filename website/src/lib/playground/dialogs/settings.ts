/**
 * Settings dialog: Qdrant endpoint + credentials, embedder provider and its
 * per-provider panels (browser model / HTTP endpoint), connection test, and
 * client reconfiguration on save.
 */

import { query, queryAll, showToast } from "../core/dom";
import { refreshStatusBar } from "../core/refresh";
import { saveSettings, state } from "../core/store";
import {
	browserModelInfo,
	DEFAULT_SETTINGS,
	type PlaygroundSettings,
} from "../core/types";
import { probeQdrant } from "../services/client";
import {
	checkEndpoint,
	configureClient,
	renderConnectionDisplay,
	setEmbedStatus,
} from "../services/connection";

type Field = HTMLInputElement | HTMLSelectElement | null;

function field(form: HTMLFormElement, name: string): Field {
	return form.elements.namedItem(name) as Field;
}

function readSettings(
	form: HTMLFormElement,
	provider: PlaygroundSettings["embedProvider"],
): PlaygroundSettings {
	const data = new FormData(form);
	const text = (name: string, fallback = ""): string =>
		String(data.get(name) ?? fallback).trim();
	return {
		qdrantUrl: text("qdrantUrl") || DEFAULT_SETTINGS.qdrantUrl,
		qdrantKey: text("qdrantKey"),
		embedProvider: provider,
		embedUrl: text("embedUrl") || DEFAULT_SETTINGS.embedUrl,
		embedModel: text("embedModel") || DEFAULT_SETTINGS.embedModel,
		embedDim: Number(data.get("embedDim")) || DEFAULT_SETTINGS.embedDim,
		embedKey: text("embedKey"),
		embedBrowserModel:
			text("embedBrowserModel") || DEFAULT_SETTINGS.embedBrowserModel,
		embedBrowserDevice:
			(data.get(
				"embedBrowserDevice",
			) as PlaygroundSettings["embedBrowserDevice"]) || "auto",
	};
}

export function setupSettingsForm(): void {
	const form = query<HTMLFormElement>("[data-settings-form]");
	if (!form) return;

	const urlInput = field(form, "qdrantUrl");
	const providerSelect = field(
		form,
		"embedProvider",
	) as HTMLSelectElement | null;
	const browserModelInput = field(form, "embedBrowserModel");
	const browserFields = query("[data-browser-embed-fields]", form);
	const httpFields = query("[data-http-embed-fields]", form);
	const browserDimsEl = query("[data-browser-model-dims]", form);

	// Populate from the persisted snapshot.
	const populateForm = (): void => {
		for (const [name, value] of Object.entries(state.settings)) {
			const input = field(form, name);
			if (!input) continue;
			input.value = String(value);
		}
		renderProviderPanels();
	};

	const renderBrowserDims = (): void => {
		if (!browserDimsEl) return;
		const model = ((browserModelInput?.value as string) ?? "").trim();
		const dims =
			state.browserDims ?? (model ? browserModelInfo(model).dims : null);
		browserDimsEl.textContent = dims
			? `${dims} dimensions reported`
			: "Reported after the first embedding";
	};

	const renderProviderPanels = (): void => {
		const provider = providerSelect?.value ?? state.settings.embedProvider;
		if (browserFields) browserFields.hidden = provider !== "browser";
		if (httpFields) httpFields.hidden = provider !== "http";
		renderBrowserDims();
	};

	providerSelect?.addEventListener("change", renderProviderPanels);
	browserModelInput?.addEventListener("input", renderBrowserDims);
	populateForm();

	queryAll("[data-open-settings]").forEach((btn) => {
		btn.addEventListener("click", populateForm);
	});

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		const provider =
			(providerSelect?.value as PlaygroundSettings["embedProvider"]) ||
			"browser";
		const next = readSettings(form, provider);
		if (provider === "browser" && !next.embedBrowserModel) {
			showToast(
				"Enter an in-browser model id, e.g. Xenova/bge-small-en-v1.5",
				true,
			);
			browserModelInput?.focus();
			return;
		}

		const modelChanged =
			next.embedBrowserModel !== state.settings.embedBrowserModel ||
			next.embedBrowserDevice !== state.settings.embedBrowserDevice;
		if (modelChanged) state.browserDims = null;
		saveSettings(next);

		setEmbedStatus(configureClient());
		renderConnectionDisplay();
		refreshStatusBar();
		form.closest("dialog")?.close();
		showToast("Settings saved");
		void checkEndpoint();
	});

	query("[data-test-connection]", form)?.addEventListener("click", () => {
		void testConnection(form, urlInput);
	});
}

async function testConnection(
	form: HTMLFormElement,
	urlInput: Field,
): Promise<void> {
	const probeEl = query("[data-embed-probe]", form);
	const probeDot = query("[data-embed-probe-dot]", form);
	if (probeEl) probeEl.textContent = "Pinging endpoint…";

	const target = (
		(urlInput?.value as string) ?? state.settings.qdrantUrl
	).trim();
	const verdict = await probeQdrant(target);

	const setDot = (cls: string): void => {
		if (!probeDot) return;
		probeDot.classList.remove(
			"bg-[var(--sl-color-gray-4)]",
			"bg-[var(--q-ok)]",
			"bg-[var(--q-bad)]",
			"bg-[var(--q-warn)]",
		);
		probeDot.classList.add(cls);
	};
	if (!probeEl) return;
	if (verdict.kind === "ok") {
		probeEl.textContent = "✓ Connected to Qdrant successfully";
		setDot("bg-[var(--q-ok)]");
	} else if (verdict.kind === "cors") {
		probeEl.textContent = "⚠ Qdrant up but blocked by browser CORS";
		setDot("bg-[var(--q-warn)]");
	} else if (verdict.kind === "http-error") {
		probeEl.textContent = `⚠ Qdrant returned HTTP ${verdict.status}`;
		setDot("bg-[var(--q-warn)]");
	} else {
		probeEl.textContent = "✕ Qdrant unreachable (connection refused)";
		setDot("bg-[var(--q-bad)]");
	}
}
