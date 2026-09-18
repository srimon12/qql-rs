/**
 * Tenant-policy dialog: the injected filter + shard clause, its recipe
 * shortcuts, and the live clause preview. Applying re-analyzes the document so
 * the plan/wire tabs show the rewritten request.
 */

import { query, queryAll, showToast } from "../core/dom";
import { savePolicy, sourceText, state } from "../core/store";
import type { RuntimePolicy } from "../core/types";
import { runAnalysis } from "../editor/analysis";

type Field = HTMLInputElement | HTMLSelectElement | null;

function field(form: HTMLFormElement, name: string): Field {
	return form.elements.namedItem(name) as Field;
}

function renderPreview(form: HTMLFormElement): void {
	const preview = query("[data-policy-preview]", form);
	if (!preview) return;
	const enabled = field(form, "enabled") as HTMLInputElement | null;
	if (!enabled?.checked) {
		preview.textContent = "Enable injection to rewrite the AST after parse.";
		return;
	}
	const type = (field(form, "valueType") as HTMLSelectElement | null)?.value;
	const value = (field(form, "value") as HTMLInputElement | null)?.value ?? "";
	const literal = type === "string" ? `"${value}"` : value;
	const target = (field(form, "field") as HTMLInputElement | null)?.value;
	const op = (field(form, "op") as HTMLSelectElement | null)?.value;
	const shard = (
		field(form, "shardKey") as HTMLInputElement | null
	)?.value.trim();
	const clause = `WHERE ${target || "tenant_id"} ${op || "="} ${literal}`;
	preview.textContent = shard ? `${clause}\nSHARD ${shard}` : clause;
}

export function setupPolicyForm(): void {
	const form = query<HTMLFormElement>("[data-policy-form]");
	if (!form) return;

	const recipes = queryAll<HTMLButtonElement>("[data-policy-recipe]", form);
	const clearRecipeActive = (): void => {
		for (const r of recipes) r.setAttribute("aria-pressed", "false");
	};

	const populateForm = (): void => {
		for (const [name, value] of Object.entries(state.policy)) {
			const input = field(form, name);
			if (!input) continue;
			if (input instanceof HTMLInputElement && input.type === "checkbox") {
				input.checked = Boolean(value);
			} else {
				input.value = String(value);
			}
		}
		clearRecipeActive();
		renderPreview(form);
	};

	for (const name of ["enabled", "field", "value", "shardKey"]) {
		field(form, name)?.addEventListener("input", () => {
			clearRecipeActive();
			renderPreview(form);
		});
	}
	for (const name of ["op", "valueType"]) {
		field(form, name)?.addEventListener("change", () => {
			clearRecipeActive();
			renderPreview(form);
		});
	}
	populateForm();

	queryAll("[data-open-policy], [data-policy-chip]").forEach((btn) => {
		btn.addEventListener("click", populateForm);
	});

	form.addEventListener("submit", (event) => {
		event.preventDefault();
		const data = new FormData(form);
		savePolicy({
			enabled:
				(field(form, "enabled") as HTMLInputElement | null)?.checked ?? false,
			field: String(data.get("field") || "tenant_id").trim(),
			op: String(data.get("op") || "="),
			value: String(data.get("value") || "").trim(),
			valueType:
				(data.get("valueType") as RuntimePolicy["valueType"]) || "string",
			shardKey: String(data.get("shardKey") || "").trim(),
		});
		form.closest("dialog")?.close();
		showToast("Tenant policy updated");
		runAnalysis(sourceText());
	});

	recipes.forEach((recipe) => {
		recipe.addEventListener("click", () => {
			clearRecipeActive();
			recipe.setAttribute("aria-pressed", "true");
			const dataset = (recipe as HTMLElement).dataset;
			const enabled = field(form, "enabled") as HTMLInputElement | null;
			if (enabled) enabled.checked = true;
			const assign = (name: string, value: string | undefined): void => {
				const input = field(form, name);
				if (input && value != null) input.value = value;
			};
			assign("field", dataset.field);
			assign("op", dataset.op);
			assign("value", dataset.value);
			assign("valueType", dataset.valueType);
			assign("shardKey", dataset.shard);
			renderPreview(form);
			showToast(`Loaded policy recipe: ${dataset.field ?? ""}`);
		});
	});
}
