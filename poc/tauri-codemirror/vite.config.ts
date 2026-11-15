import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: "TAURI_",
  build: {
    target: "es2022",
    outDir: "dist",
  },
});
