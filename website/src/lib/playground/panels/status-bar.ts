/**
 * Header status bar: connection + runtime + embedder pills, the tenant-policy
 * chip, the validation badge, statement count, and the analysis summary line.
 */
import { query } from "../core/dom";
import { state, statementCount } from "../core/store";
import { browserModelInfo } from "../core/types";
import { selectedRoute } from "../core/wasm";

export function renderStatusBar(): void {
	const validationBadge = query("[data-validation-badge]");
	const summary = query("[data-analysis-summary]");
	const stmtCount = query("[data-stmt-count]");
	const policyDot = query("[data-policy-dot]");
	const policyChip = query("[data-policy-chip]");
	const policyChipValue = query("[data-policy-chip-value]");
	const embedChip = query("[data-status-embed]");
	const embedDimChip = query("[data-embed-dim-chip]");

	if (embedChip) {
		if (state.settings.embedProvider === "browser") {
			embedChip.textContent = `${browserModelInfo(state.settings.embedBrowserModel).label} (browser)`;
		} else if (state.settings.embedProvider === "http") {
			embedChip.textContent = state.settings.embedModel || "HTTP";
		} else {
			embedChip.textContent = "None (raw vectors)";
		}
	}
	if (embedDimChip) {
		embedDimChip.hidden = state.settings.embedProvider === "none";
		if (state.settings.embedProvider === "browser") {
			const dims =
				state.browserDims ??
				browserModelInfo(state.settings.embedBrowserModel).dims;
			embedDimChip.textContent = dims ? `${dims}d` : "auto";
		} else {
			embedDimChip.textContent = `${state.settings.embedDim}d`;
		}
	}

	if (policyDot) {
		policyDot.classList.toggle("is-active", state.policy.enabled);
	}
	if (policyChip && policyChipValue) {
		policyChip.hidden = !state.policy.enabled;
		policyChipValue.textContent = `${state.policy.field} ${state.policy.op} ${state.policy.value}`;
	}

	const count = statementCount();
	if (stmtCount && count > 0) {
		stmtCount.textContent = `${count} ${count === 1 ? "stmt" : "stmts"}`;
	}

	const analysis = state.analysis;
	if (!analysis) return;

	if (validationBadge) {
		validationBadge.classList.remove("is-valid", "is-invalid");
		if (analysis.result.valid && !analysis.policyError) {
			validationBadge.classList.add("is-valid");
			validationBadge.innerHTML = `<span class="size-1.5 rounded-full bg-[var(--q-ok)]"></span> Valid QQL`;
		} else {
			validationBadge.classList.add("is-invalid");
			validationBadge.innerHTML = `<span class="size-1.5 rounded-full bg-[var(--q-bad)]"></span> Syntax error`;
		}
	}

	if (summary) {
		if (analysis.policyError) {
			summary.textContent = analysis.policyError;
		} else if (!analysis.result.valid) {
			const err = analysis.result.error;
			summary.textContent = err
				? `${err.code}: ${err.message}`
				: "Syntax error in statement";
		} else {
			const route = selectedRoute(analysis, state.selectedStatement);
			summary.textContent = `Ready · ${route?.method ?? "POST"} ${route?.path ?? ""}`;
		}
	}
}
