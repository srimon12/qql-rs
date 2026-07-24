import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      // Consume wasm-pack output directly. pnpm copies `file:` dependencies
      // into its store, which can leave Vite running an older WASM binary.
      "qql-wasm": path.resolve(__dirname, "../demo/pkg/qql_wasm.js"),
    },
  },
  optimizeDeps: {
    exclude: ["qql-wasm", "@huggingface/transformers"],
  },
  server: {
    fs: {
      // allow linking local ../demo/pkg wasm artifacts
      allow: [path.resolve(__dirname, "..")],
    },
    headers: {
      // Required for multi-threaded WASM (onnxruntime) when COOP/COEP used;
      // safe defaults for Transformers.js single-thread path.
      "Cross-Origin-Embedder-Policy": "credentialless",
      "Cross-Origin-Opener-Policy": "same-origin",
    },
  },
  assetsInclude: ["**/*.wasm"],
  worker: {
    format: "es",
  },
})
