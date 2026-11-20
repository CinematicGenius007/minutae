import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Standalone preview of the component gallery: `bun run dev`.
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [react()],
  server: { port: 5199, strictPort: false },
  build: { outDir: fileURLToPath(new URL("../dist-gallery", import.meta.url)), emptyOutDir: true },
});
