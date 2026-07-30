// Expressive Code configuration for the QQL documentation site.
//
// The site uses Markdoc (not standard Astro Markdown), so code blocks are
// rendered via Starlight's <Code> component rather than through the rehype
// pipeline. The standard postprocessRenderedBlockGroup hook that injects
// the theme CSS checks for `groupIndex === 0`, which is undefined for
// directly-rendered blocks. This config fixes both issues:
//
// 1. emitExternalStylesheet: false → use inline <style> tags instead of
//    <link> references, sidestepping dev-mode virtual module resolution.
// 2. A custom plugin ensures the first block in any document always has
//    groupIndex 0 so the standard CSS injection hook fires.

export default {
  /** Inline the EC CSS so it works with <Code> component direct rendering. */
  emitExternalStylesheet: false,

  plugins: [
    {
      name: "ensure-first-group",
      hooks: {
        postprocessRenderedBlockGroup({ renderedGroupContents }) {
          // Blocks rendered via <Code> component don't have a
          // positionInDocument set, which prevents the standard
          // astro-expressive-code plugin from injecting the theme CSS.
          for (const { codeBlock } of renderedGroupContents) {
            if (!codeBlock.parentDocument) {
              codeBlock.parentDocument = {};
            }
            if (!codeBlock.parentDocument.positionInDocument) {
              codeBlock.parentDocument.positionInDocument = { groupIndex: 0 };
            }
          }
        },
      },
    },
  ],
};
