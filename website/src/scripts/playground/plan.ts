import { selectedRoute } from "./analysis";
import { h, required } from "./dom";
import { renderOutputs, renderValidation } from "./inspector";
import { store } from "./store";

const state = store.state;

const statementSelect = required<HTMLSelectElement>("[data-statement-select]");
const planEmpty = required<HTMLElement>("[data-empty-plan]");
const planCard = required<HTMLElement>("[data-plan-card]");
const routeMethod = required<HTMLElement>("[data-route-method]");
const routePath = required<HTMLElement>("[data-route-path]");
const routeType = required<HTMLElement>("[data-route-type]");
const routePolicy = required<HTMLElement>("[data-route-policy]");
const routesSection = required<HTMLElement>("[data-routes-section]");
const routesList = required<HTMLElement>("[data-routes-list]");
const routesCount = required<HTMLElement>("[data-routes-count]");

export function statementValue(): number {
	return Number(statementSelect.value);
}

export function onStatementChange(run: () => void): void {
	statementSelect.addEventListener("change", () => {
		run();
	});
}

function renderStatementSelect(): void {
	const analysis = state.analysis;
	const count = Math.max(
		analysis?.result.statements_count ?? 0,
		analysis?.effectiveRoutes.length ?? 0,
	);
	statementSelect.replaceChildren();
	for (let index = 0; index < count; index += 1) {
		const option = document.createElement("option");
		option.value = String(index);
		option.textContent = `Statement ${index + 1}`;
		statementSelect.append(option);
	}
	state.selectedStatement = Math.min(
		state.selectedStatement,
		Math.max(0, count - 1),
	);
	statementSelect.value = String(state.selectedStatement);
	statementSelect.disabled = count <= 1;
	const single = count <= 1;
	required<HTMLElement>("[data-statement-wrap]").hidden = single;
	statementSelect.hidden = single;
}

function shortRoutePath(path: string): string {
	const parts = path.split("/").filter(Boolean);
	return parts.length > 2 ? `/${parts.slice(-2).join("/")}` : path;
}

function renderPlan(): void {
	const route = selectedRoute(state.analysis, state.selectedStatement);
	planEmpty.hidden = Boolean(route);
	planCard.hidden = !route;
	const routeLine = required<HTMLElement>("[data-route-line]");
	if (!route) {
		routeLine.textContent = "";
		return;
	}
	routeMethod.textContent = route.method;
	routePath.textContent = route.path;
	routeType.textContent = route.stmt_type;
	routePolicy.textContent = store.policy.enabled
		? "Trusted predicate injected"
		: "Source only";
	routeLine.textContent = `${route.method} ${shortRoutePath(route.path)}`;
}

function renderRoutes(): void {
	const routes = state.analysis?.effectiveRoutes ?? [];
	routesSection.hidden = routes.length === 0;
	routesCount.textContent = `${routes.length} ${routes.length === 1 ? "statement" : "statements"}`;
	routesList.replaceChildren();

	routes.forEach((route, index) => {
		const item = h(
			"button",
			{
				type: "button",
				className: "route-row",
				role: "option",
				ariaSelected: String(index === state.selectedStatement),
				ariaCurrent: String(index === state.selectedStatement),
				dataset: { routeIndex: String(index) },
				onClick: () => selectStatement(index),
			},
			h("span", { className: "route-row__index" }, String(index + 1)),
			h("span", { className: "route-row__type" }, route.stmt_type),
			h("span", { className: "route-row__method" }, route.method),
			h("span", { className: "route-row__path" }, route.path),
		);
		routesList.append(item);
	});
}

/** Insert `USING <vector> AS DENSE` after FROM — explicit user confirmation. */
export function selectStatement(index: number): void {
	state.selectedStatement = index;
	statementSelect.value = String(index);
	renderPlan();
	renderOutputs();
	renderRoutes();
}

export function renderInspector(): void {
	renderStatementSelect();
	renderPlan();
	renderOutputs();
	renderRoutes();
	renderValidation();
}
