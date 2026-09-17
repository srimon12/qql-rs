/**
 * UI refresh hub. Every state mutation funnels through one of these calls so
 * the three chrome surfaces (status bar, statement chrome, inspector) render
 * from the same snapshot, in one pass, without cross-module callbacks.
 */
import { renderInspector } from "./inspector";
import { renderRunMenuHint, renderStatementRail } from "./statement-nav";
import { renderStatusBar } from "./status-bar";

/** Editor strip + rail: selection and per-statement status changed. */
export function refreshStatementUi(): void {
	renderStatementRail();
	renderRunMenuHint();
}

/** Inspector: plan, code tabs, response, and route/statement marks. */
export function refreshInspector(): void {
	renderInspector();
}

/** Header status bar: connection, embedder, validation, statement count. */
export function refreshStatusBar(): void {
	renderStatusBar();
}

/** Everything, for a fresh analysis or document swap. */
export function refreshAll(): void {
	refreshStatusBar();
	refreshStatementUi();
	refreshInspector();
}
