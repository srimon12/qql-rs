import { defineMarkdocConfig, component } from "@astrojs/markdoc/config";
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
      render: component("@qql/ui-docs/TechBadge.astro"),
      attributes: {
        name: { type: String, required: true },
        variant: { type: String },
        color: { type: String },
      },
    },
    apiField: {
      render: component("@qql/ui-docs/ApiField.astro"),
      attributes: {
        name: { type: String, required: true },
        type: { type: String, required: true },
        required: { type: Boolean, default: false },
      },
    },
    glassCard: {
      render: component("@qql/ui-docs/GlassCard.astro"),
      attributes: {
        title: { type: String },
      },
    },
    kbd: {
      render: component("@qql/ui-docs/Kbd.astro"),
      attributes: {
        keyName: { type: String, required: true },
      },
    },
    terminal: {
      render: component("@qql/ui-docs/Terminal.astro"),
      attributes: {
        title: { type: String },
      },
    },
    packageManager: {
      render: component("@qql/ui-docs/PackageManager.astro"),
      attributes: {
        cmd: { type: String, required: true },
        dev: { type: Boolean, default: false },
      },
    },
  },
});
