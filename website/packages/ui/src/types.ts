export interface DocsNavItem {
	name: string;
	href: string;
	external?: boolean;
}

export interface DocsFooterColumn {
	title: string;
	items: {
		label: string;
		href: string;
		external?: boolean;
		disabled?: boolean;
	}[];
}

export interface DocsFooterConfig {
	brand: {
		name: string;
		href: string;
		tagline: string;
	};
	columns: DocsFooterColumn[];
	bottom?: {
		copyright: string;
		note?: string;
	};
}

export interface DocsHeaderConfig {
	navItems: DocsNavItem[];
}
