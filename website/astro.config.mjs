// @ts-check

import markdoc from "@astrojs/markdoc";
import sitemap from "@astrojs/sitemap";
import starlight from "@astrojs/starlight";
import starlightCopyButton from "starlight-copy-button";
import starlightLlmsTxt from "starlight-llms-txt";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";
import { fileURLToPath } from "node:url";

const SITE_URL = "https://qql.veristamp.in";

// Tailwind v4 plugin returns Plugin[] which Vite's PluginOption type rejects under Astro 7.
const tailwind = /** @type {any} */ (tailwindcss());

export default defineConfig(
  /** @type {import("astro").AstroUserConfig} */ ({
    site: SITE_URL,
    trailingSlash: "always",

    integrations: [
      sitemap({
        filter: (page) => !page.includes("/docs/landing/"),
      }),
      starlight({
        plugins: [
          starlightCopyButton(),
          starlightLlmsTxt({ projectName: "QQL" }),
        ],
        title: "QQL Documentation",
        favicon: "/favicon.ico",
        description:
          "Declarative vector search for Qdrant — QQL language, SDKs, CLI, and security patterns.",
        social: [
          {
            icon: "github",
            label: "GitHub",
            href: "https://github.com/srimon12/qql-rs",
          },
        ],
        customCss: [
          "@fontsource/instrument-serif/index.css",
          "@fontsource/ibm-plex-sans/400.css",
          "@fontsource/ibm-plex-sans/500.css",
          "@fontsource/ibm-plex-sans/600.css",
          "@fontsource/jetbrains-mono/400.css",
          "@fontsource/jetbrains-mono/500.css",
          "./src/styles/global.css",
        ],
        components: {
          Footer: "./src/components/DocsFooter.astro",
          Header: "./src/components/DocsHeader.astro",
        },
        sidebar: [
          {
            label: "Start",
            items: [
              { label: "Overview", link: "/docs/" },
              { label: "What is QQL?", link: "/docs/getting-started/" },
              {
                label: "Installation",
                link: "/docs/getting-started/installation/",
              },
              {
                label: "Quickstart",
                link: "/docs/getting-started/quickstart/",
              },
              {
                label: "Execution model",
                link: "/docs/getting-started/execution-model/",
              },
            ],
          },
          {
            label: "Language",
            items: [{ autogenerate: { directory: "docs/language" } }],
          },
          {
            label: "Guides",
            items: [{ autogenerate: { directory: "docs/guides" } }],
          },
          {
            label: "SDKs",
            items: [{ autogenerate: { directory: "docs/sdks" } }],
          },
          {
            label: "Tools",
            items: [{ autogenerate: { directory: "docs/tools" } }],
          },
          {
            label: "Reference",
            items: [{ autogenerate: { directory: "docs/reference" } }],
          },
          {
            label: "Contributing",
            items: [{ autogenerate: { directory: "docs/contributing" } }],
          },
        ],
      }),
      markdoc(),
    ],
    vite: {
      plugins: [tailwind],
      resolve: {
        alias: {
          "qql-wasm-current": fileURLToPath(
            new URL("./.wasm/qql-wasm/qql_wasm.js", import.meta.url),
          ),
        },
      },
    },
  }),
);
