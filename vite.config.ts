import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { codeInspectorPlugin } from "code-inspector-plugin";

// Vite resolves module ids through realpath (junction-aware), so root must use
// the same real path to avoid mixing path variants on Windows junctions.
const root = fs.realpathSync(path.resolve(__dirname, "src"));

export default defineConfig(({ command }) => ({
  root,
  plugins: [
    command === "serve" &&
      codeInspectorPlugin({
        bundler: "vite",
      }),
    react(),
  ].filter(Boolean),
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
  },
  server: {
    port: 3000,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": root,
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
}));
