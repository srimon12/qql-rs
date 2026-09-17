import type {
	InspectorTab,
	PlaygroundSettings,
	PlaygroundState,
	RuntimePolicy,
} from "../playground-types";
import { DEFAULT_POLICY, DEFAULT_SETTINGS } from "../playground-types";

export const SETTINGS_KEY = "qql-playground.settings.v1";
export const POLICY_KEY = "qql-playground.policy.v2";
export const WORKSPACE_KEY = "qql-playground.workspace.v1";
export const INSPECTOR_TAB_KEY = "qql-playground.inspector-tab.v1";
export const WRAP_KEY = "qql-playground.wrap.v1";
export const SPLIT_KEY = "qql-playground.split.v1";
export const FIRST_RUN_KEY = "pg-first-run";

export function loadStored<T>(key: string, fallback: T): T {
	try {
		const raw = localStorage.getItem(key);
		if (!raw) return { ...fallback };
		return { ...fallback, ...(JSON.parse(raw) as Partial<T>) };
	} catch {
		return { ...fallback };
	}
}

export function loadSession(key: string): string | null {
	try {
		return sessionStorage.getItem(key);
	} catch {
		return null;
	}
}

export function saveSession(key: string, value: string): void {
	try {
		sessionStorage.setItem(key, value);
	} catch {
		// Private browsing may deny storage; the playground remains usable.
	}
}

function persist(key: string, value: unknown): void {
	try {
		localStorage.setItem(key, JSON.stringify(value));
	} catch {
		// Private browsing may deny storage; the session still uses the values.
	}
}

export interface PlaygroundStore {
	state: PlaygroundState;
	settings: PlaygroundSettings;
	policy: RuntimePolicy;
	/** Last Qdrant verdict: drives EnvPill + offline empty states. */
	qdrant: "unknown" | "ok" | "down";
	noteQdrant(ok: boolean): void;
	saveSettings(next: PlaygroundSettings): void;
	savePolicy(next: RuntimePolicy): void;
}

/** First successful run happened (hides the first-run ping + skeleton). */
export function hasRunOnce(): boolean {
	try {
		return localStorage.getItem(FIRST_RUN_KEY) === "1";
	} catch {
		return false;
	}
}

export function markRunOnce(): void {
	try {
		localStorage.setItem(FIRST_RUN_KEY, "1");
	} catch {
		// Private browsing may deny storage; the ping simply persists.
	}
}

/** Restored tab, defaulting to Result (hits first, compiler one click away). */
export function resolveInitialTab(): InspectorTab {
	const saved = loadSession(INSPECTOR_TAB_KEY);
	return saved === "plan" ||
		saved === "response" ||
		saved === "wire" ||
		saved === "ast" ||
		saved === "tokens" ||
		saved === "explain" ||
		saved === "metrics"
		? saved
		: "response";
}

export function createStore(): PlaygroundStore {
	const state: PlaygroundState = {
		analysis: null,
		response: null,
		executionError: null,
		selectedStatement: 0,
		inspectorTab: resolveInitialTab(),
		exportLanguage: "python",
		metrics: null,
	};
	const settings = loadStored(SETTINGS_KEY, DEFAULT_SETTINGS);
	const policy = loadStored(POLICY_KEY, DEFAULT_POLICY);
	const store: PlaygroundStore = {
		state,
		settings,
		policy,
		qdrant: "unknown",
		noteQdrant(ok) {
			store.qdrant = ok ? "ok" : "down";
		},
		saveSettings(next) {
			store.settings = next;
			persist(SETTINGS_KEY, next);
		},
		savePolicy(next) {
			store.policy = next;
			persist(POLICY_KEY, next);
		},
	};
	return store;
}

/** Shared instance: every module reads the same state. */
export const store = createStore();
