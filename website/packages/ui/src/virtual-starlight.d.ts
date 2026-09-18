/**
 * Ambient declarations for the Starlight virtual modules `@qql/ui` renders.
 *
 * `virtual:starlight/components/*` resolves to the *effective* component — the
 * consumer's override when one exists, otherwise Starlight's built-in — which
 * is what keeps the rest of Starlight's chrome replaceable while these
 * components stand in for `Header` and `Footer`. Upstream maintains these
 * declarations (`virtual.d.ts`) in its repository, but `@astrojs/starlight@0.42.x`
 * publishes only `dist/` (`files: ["dist"]`), so `astro check` cannot resolve
 * the imports. This file mirrors upstream's declarations, scoped to the modules
 * used here; each component keeps the exact props of its public built-in
 * counterpart, and delete these the day upstream ships its own again.
 */
declare module "virtual:starlight/user-config" {
	const config: import("@astrojs/starlight/types").StarlightConfig;
	export default config;
}

declare module "virtual:starlight/components/LanguageSelect" {
	const LanguageSelect: typeof import("@astrojs/starlight/components/LanguageSelect.astro").default;
	export default LanguageSelect;
}

declare module "virtual:starlight/components/Search" {
	const Search: typeof import("@astrojs/starlight/components/Search.astro").default;
	export default Search;
}

declare module "virtual:starlight/components/SocialIcons" {
	const SocialIcons: typeof import("@astrojs/starlight/components/SocialIcons.astro").default;
	export default SocialIcons;
}

declare module "virtual:starlight/components/EditLink" {
	const EditLink: typeof import("@astrojs/starlight/components/EditLink.astro").default;
	export default EditLink;
}

declare module "virtual:starlight/components/LastUpdated" {
	const LastUpdated: typeof import("@astrojs/starlight/components/LastUpdated.astro").default;
	export default LastUpdated;
}

declare module "virtual:starlight/components/Pagination" {
	const Pagination: typeof import("@astrojs/starlight/components/Pagination.astro").default;
	export default Pagination;
}
