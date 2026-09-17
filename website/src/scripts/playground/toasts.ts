import { h } from "./dom";
import type { PlaygroundFailure } from "../playground-types";

const TOAST_LIFE_MS = 3600;
const TOAST_ERROR_LIFE_MS = 5600;

let region: HTMLElement | null = null;

/** Bind once at boot; toast() keeps its call sites unchanged. */
export function initToasts(toastRegion: HTMLElement): void {
	region = toastRegion;
}

function append(item: HTMLElement, lifeMs: number): void {
	region?.append(item);
	window.setTimeout(() => item.remove(), lifeMs);
}

export function toast(
	message: string,
	tone: "success" | "error" = "success",
): void {
	append(
		h("div", { className: `toast toast--${tone}`, text: message }),
		TOAST_LIFE_MS,
	);
}

/** Human sentence plus the parsed error code; raw JSON stays in the tooltip. */
export function toastFailure(failure: PlaygroundFailure): void {
	append(
		h(
			"div",
			{
				className: "toast toast--error",
				title: failure.code ? `${failure.code}: ${failure.raw}` : failure.raw,
			},
			h("span", null, failure.message),
			failure.code ? " " : null,
			failure.code
				? h("code", {
						className:
							"rounded border border-[var(--sl-color-gray-5)] px-1 font-mono text-[0.65rem] whitespace-nowrap text-[var(--sl-color-gray-3)]",
						text: failure.code,
					})
				: null,
		),
		TOAST_ERROR_LIFE_MS,
	);
}
