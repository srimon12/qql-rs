import { component, defineMarkdocConfig } from "@astrojs/markdoc/config";
import starlightMarkdoc from "@astrojs/starlight-markdoc";

export default defineMarkdocConfig({
	extends: [starlightMarkdoc()],
	tags: {
		grid: {
			render: "div",
			attributes: {
				style: { type: String },
			},
		},
		techBadge: {
			render: component("@qql/ui/TechBadge.astro"),
			attributes: {
				name: { type: String, required: true },
				variant: { type: String },
				color: { type: String },
			},
		},
		apiField: {
			render: component("@qql/ui/ApiField.astro"),
			attributes: {
				name: { type: String, required: true },
				type: { type: String, required: true },
				required: { type: Boolean, default: false },
			},
		},
		glassCard: {
			render: component("@qql/ui/GlassCard.astro"),
			attributes: {
				title: { type: String },
			},
		},
		qqlExample: {
			render: component("@qql/ui/QqlExample.astro"),
			attributes: {
				title: { type: String },
			},
		},
		terminal: {
			render: component("@qql/ui/Terminal.astro"),
			attributes: {
				title: { type: String },
			},
		},
		faqList: {
			render: component("./src/components/FaqList.astro"),
			attributes: {},
		},
	},
});
