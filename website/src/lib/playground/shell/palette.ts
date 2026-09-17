/**
 * Command palette for the playground (⌘K / Ctrl+K).
 *
 * One searchable surface for every playground action: run targets, statement
 * jumps, editor tools, and dialogs. Markup lives in the page; this module only
 * renders the filtered list and owns keyboard navigation.
 */

import { escapeHtml } from "../core/dom";

export interface PaletteCommand {
	group: string;
	label: string;
	detail?: string;
	hint?: string;
	run: () => void;
}

export interface CommandPalette {
	open(): void;
	close(): void;
	toggle(): void;
	readonly isOpen: boolean;
}

export interface CommandPaletteOptions {
	dialog: HTMLDialogElement;
	getCommands: () => PaletteCommand[];
}

export function setupCommandPalette(
	options: CommandPaletteOptions,
): CommandPalette {
	const { dialog, getCommands } = options;
	const input = dialog.querySelector<HTMLInputElement>("[data-palette-input]");
	const list = dialog.querySelector<HTMLElement>("[data-palette-list]");
	const empty = dialog.querySelector<HTMLElement>("[data-palette-empty]");

	let visible: PaletteCommand[] = [];
	let activeIndex = 0;

	const matches = (command: PaletteCommand, term: string): boolean => {
		if (!term) return true;
		const haystack =
			`${command.group} ${command.label} ${command.detail ?? ""}`.toLowerCase();
		return term.split(/\s+/).every((token) => haystack.includes(token));
	};

	const setActive = (index: number): void => {
		if (visible.length === 0) return;
		activeIndex = (index + visible.length) % visible.length;
		const items = list?.querySelectorAll<HTMLButtonElement>(
			"[data-palette-item]",
		);
		items?.forEach((item, itemIndex) => {
			const isActive = itemIndex === activeIndex;
			item.setAttribute("aria-selected", String(isActive));
			if (isActive) {
				item.scrollIntoView({ block: "nearest" });
				input?.setAttribute("aria-activedescendant", item.id);
			}
		});
	};

	const render = (term: string): void => {
		if (!list) return;
		const commands = getCommands();
		visible = commands.filter((command) => matches(command, term));
		activeIndex = 0;
		if (visible.length === 0) {
			list.innerHTML = "";
			if (empty) empty.hidden = false;
			return;
		}
		if (empty) empty.hidden = true;
		let currentGroup = "";
		const parts: string[] = [];
		visible.forEach((command, index) => {
			if (command.group !== currentGroup) {
				currentGroup = command.group;
				parts.push(
					`<li class="px-2 pb-1 pt-2.5 font-mono text-[0.6rem] font-bold uppercase tracking-[0.12em] text-[var(--sl-color-gray-3)]" role="presentation">${escapeHtml(command.group)}</li>`,
				);
			}
			parts.push(`
				<li role="presentation">
					<button type="button" id="palette-item-${index}" data-palette-item="${index}" role="option" aria-selected="false"
						class="flex w-full items-center justify-between gap-3 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-[var(--sl-color-gray-6)] aria-selected:bg-[var(--sl-color-gray-6)]">
						<span class="grid min-w-0 gap-0.5">
							<span class="truncate text-[0.8125rem] font-medium text-[var(--sl-color-gray-1)]">${escapeHtml(command.label)}</span>
							${command.detail ? `<span class="truncate font-mono text-[0.65rem] text-[var(--sl-color-gray-3)]">${escapeHtml(command.detail)}</span>` : ""}
						</span>
						${command.hint ? `<kbd class="shrink-0 rounded border border-[var(--sl-color-gray-5)] bg-[var(--sl-color-gray-6)] px-1.5 py-0.5 font-mono text-[0.62rem] text-[var(--sl-color-gray-3)]">${escapeHtml(command.hint)}</kbd>` : ""}
					</button>
				</li>`);
		});
		list.innerHTML = parts.join("");
		setActive(0);
	};

	const runCommand = (index: number): void => {
		const command = visible[index];
		if (!command) return;
		close();
		// Let the dialog finish closing before the action moves focus/DOM.
		window.setTimeout(() => command.run(), 0);
	};

	const open = (): void => {
		if (dialog.open) return;
		render("");
		dialog.showModal();
		input?.focus();
	};

	const close = (): void => {
		if (dialog.open) dialog.close();
	};

	input?.addEventListener("input", () =>
		render(input.value.toLowerCase().trim()),
	);
	input?.addEventListener("keydown", (event) => {
		if (event.key === "ArrowDown") {
			event.preventDefault();
			setActive(activeIndex + 1);
		} else if (event.key === "ArrowUp") {
			event.preventDefault();
			setActive(activeIndex - 1);
		} else if (event.key === "Enter") {
			event.preventDefault();
			runCommand(activeIndex);
		}
	});
	list?.addEventListener("click", (event) => {
		const item = (event.target as HTMLElement).closest<HTMLButtonElement>(
			"[data-palette-item]",
		);
		if (item?.dataset.paletteItem) {
			runCommand(Number(item.dataset.paletteItem));
		}
	});
	list?.addEventListener("mousemove", (event) => {
		const item = (event.target as HTMLElement).closest<HTMLButtonElement>(
			"[data-palette-item]",
		);
		if (item?.dataset.paletteItem) {
			const index = Number(item.dataset.paletteItem);
			if (index !== activeIndex) {
				setActive(index);
			}
		}
	});
	dialog.addEventListener("click", (event) => {
		if (event.target === dialog) close();
	});
	dialog.addEventListener("close", () => {
		if (input) input.value = "";
	});

	return {
		open,
		close,
		toggle: () => (dialog.open ? close() : open()),
		get isOpen() {
			return dialog.open;
		},
	};
}
