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
    },
  },
  optimizeDeps: {
    exclude: ["qql-wasm"],
  },
  server: {
    fs: {
      // allow linking local ../demo/pkg wasm artifacts
      allow: [path.resolve(__dirname, "..")],
    },
  },
  assetsInclude: ["**/*.wasm"],
})
