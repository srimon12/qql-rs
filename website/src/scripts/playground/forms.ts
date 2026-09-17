import type { EditorView } from "@codemirror/view";

import { clearTopologyCache, pool } from "./client";
import { loadLivePresets } from "./presets";
import { all, required } from "./dom";
import { formatError } from "./errors";

import type {
	ExportLanguage,
	PlaygroundSettings,
	PolicyValueType,
} from "../playground-types";

import {
	setRuntime,
	syncPolicyChip,
	updateConnectionSummary,
} from "./statusbar";
import { store } from "./store";
import { setupExporter } from "./exporter";
import { toast } from "./toasts";

export interface FormDeps {
	runAnalysis: (source: string) => void;
	checkEndpoint: (manual?: boolean) => void;
}

let activeEditor: EditorView | null = null;

export function initForms(editor: EditorView, deps: FormDeps): void {
	activeEditor = editor;
	writeSettingsForm(deps);
	writePolicyForm(deps);
	setupExporter(editor);
}

function editorSource(): string {
	return activeEditor?.state.doc.toString() ?? "";
}

const embedStatus = required<HTMLElement>("[data-embed-status]");

export function configureClient(): void {
	try {
		embedStatus.textContent = pool.configure(store.settings, (message) => {
			embedStatus.textContent = message;
			setRuntime(message);
		});
	} finally {
		updateConnectionSummary(store.settings);
	}
}
function writeSettingsForm(deps: FormDeps): void {
	const { runAnalysis, checkEndpoint } = deps;
	const form = required<HTMLFormElement>("[data-settings-form]");
	const field = <T extends HTMLInputElement | HTMLSelectElement>(
		name: string,
	) => required<T>(`[data-settings-form] [name="${name}"]`);
	field<HTMLInputElement>("qdrantUrl").value = store.settings.qdrantUrl;
	field<HTMLInputElement>("qdrantKey").value = store.settings.qdrantKey;
	field<HTMLSelectElement>("embedProvider").value =
		store.settings.embedProvider;
	field<HTMLInputElement>("embedUrl").value = store.settings.embedUrl;
	field<HTMLInputElement>("embedModel").value = store.settings.embedModel;
	field<HTMLInputElement>("embedDim").value = String(store.settings.embedDim);
	field<HTMLInputElement>("embedKey").value = store.settings.embedKey;

	const toggleHttpFields = () => {
		required<HTMLFieldSetElement>("[data-http-embed-fields]").hidden =
			field<HTMLSelectElement>("embedProvider").value !== "http";
	};
	field<HTMLSelectElement>("embedProvider").addEventListener(
		"change",
		toggleHttpFields,
	);
	toggleHttpFields();

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		store.saveSettings({
			qdrantUrl: field<HTMLInputElement>("qdrantUrl").value.replace(/\/+$/, ""),
			qdrantKey: field<HTMLInputElement>("qdrantKey").value,
			embedProvider: field<HTMLSelectElement>("embedProvider")
				.value as PlaygroundSettings["embedProvider"],
			embedUrl: field<HTMLInputElement>("embedUrl").value,
			embedModel: field<HTMLInputElement>("embedModel").value,
			embedDim: Number(field<HTMLInputElement>("embedDim").value),
			embedKey: field<HTMLInputElement>("embedKey").value,
		});
		try {
			configureClient();
			required<HTMLDialogElement>("#settings-dialog").close();
			clearTopologyCache();
			runAnalysis(editorSource());
			void checkEndpoint();
			void loadLivePresets();
			toast("Connection settings saved.");
		} catch (error) {
			toast(formatError(error), "error");
		}
	});
	required<HTMLButtonElement>("[data-test-connection]").addEventListener(
		"click",
		() => void checkEndpoint(true),
	);
}
function writePolicyForm(deps: FormDeps): void {
	const { runAnalysis } = deps;
	const form = required<HTMLFormElement>("[data-policy-form]");
	const field = <T extends HTMLInputElement | HTMLSelectElement>(
		name: string,
	) => required<T>(`[data-policy-form] [name="${name}"]`);
	const recipes = all<HTMLButtonElement>("[data-policy-recipe]");
	field<HTMLInputElement>("enabled").checked = store.policy.enabled;
	field<HTMLInputElement>("field").value = store.policy.field;
	field<HTMLSelectElement>("op").value = store.policy.op;
	field<HTMLInputElement>("value").value = store.policy.value;
	field<HTMLSelectElement>("valueType").value = store.policy.valueType;
	field<HTMLInputElement>("shardKey").value = store.policy.shardKey;
	syncPolicyChip(store.policy);

	const preview = required<HTMLOutputElement>("[data-policy-preview]");
	const quotePreviewValue = (raw: string, valueType: string) => {
		if (valueType === "string") return `'${raw || "value"}'`;
		return raw || "value";
	};
	const syncRecipeSelection = () => {
		const current = {
			field: field<HTMLInputElement>("field").value.trim(),
			op: field<HTMLSelectElement>("op").value,
			value: field<HTMLInputElement>("value").value,
			valueType: field<HTMLSelectElement>("valueType").value,
			shard: field<HTMLInputElement>("shardKey").value.trim(),
		};
		for (const recipe of recipes) {
			const matches =
				recipe.dataset.field === current.field &&
				(recipe.dataset.op ?? "=") === current.op &&
				recipe.dataset.value === current.value &&
				(recipe.dataset.valueType ?? "string") === current.valueType &&
				(recipe.dataset.shard ?? "") === current.shard;
			recipe.setAttribute("aria-pressed", String(matches));
		}
	};
	const updatePreview = () => {
		const enabled = field<HTMLInputElement>("enabled").checked;
		const fieldName = field<HTMLInputElement>("field").value.trim() || "field";
		const operator = field<HTMLSelectElement>("op").value;
		const valueType = field<HTMLSelectElement>("valueType").value;
		const value = quotePreviewValue(
			field<HTMLInputElement>("value").value,
			valueType,
		);
		const shard = field<HTMLInputElement>("shardKey").value.trim();
		if (!enabled) {
			preview.textContent =
				"The source stays unchanged. Enable injection to rewrite the AST after parse.";
			preview.classList.remove("text-[var(--q-ok)]");
			syncRecipeSelection();
			return;
		}
		const parts = [`WHERE ${fieldName} ${operator} ${value}`];
		if (shard) parts.push(`SHARD '${shard}'`);
		preview.textContent = `+ ${parts.join("\n+ ")}`;
		preview.classList.add("text-[var(--q-ok)]");
		syncRecipeSelection();
	};
	updatePreview();

	for (const name of [
		"enabled",
		"field",
		"op",
		"value",
		"valueType",
		"shardKey",
	] as const) {
		field<HTMLInputElement | HTMLSelectElement>(name).addEventListener(
			"input",
			updatePreview,
		);
		field<HTMLInputElement | HTMLSelectElement>(name).addEventListener(
			"change",
			updatePreview,
		);
	}

	for (const recipe of recipes) {
		recipe.addEventListener("click", () => {
			field<HTMLInputElement>("enabled").checked = true;
			field<HTMLInputElement>("field").value = recipe.dataset.field ?? "";
			field<HTMLSelectElement>("op").value = recipe.dataset.op ?? "=";
			field<HTMLInputElement>("value").value = recipe.dataset.value ?? "";
			field<HTMLSelectElement>("valueType").value =
				recipe.dataset.valueType ?? "string";
			field<HTMLInputElement>("shardKey").value = recipe.dataset.shard ?? "";
			updatePreview();
		});
	}

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		store.savePolicy({
			enabled: field<HTMLInputElement>("enabled").checked,
			field: field<HTMLInputElement>("field").value.trim(),
			op: field<HTMLSelectElement>("op").value,
			value: field<HTMLInputElement>("value").value,
			valueType: field<HTMLSelectElement>("valueType").value as PolicyValueType,
			shardKey: field<HTMLInputElement>("shardKey").value,
		});
		syncPolicyChip(store.policy);
		required<HTMLDialogElement>("#policy-dialog").close();
		runAnalysis(editorSource());
		toast(
			store.policy.enabled
				? "Runtime policy applied."
				: "Runtime policy disabled.",
		);
	});
}
let exportEditor: EditorView | null = null;

