import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
const root = path.resolve(__dirname, "../..");
export default defineConfig({
  root: __dirname,
  plugins: [react()],
  resolve: {
    alias: [
      {
        find: "@/hooks/useSidebarStatus",
        replacement: path.join(__dirname, "fixtures.ts"),
      },
      {
        find: "@/contexts/UpdateContext",
        replacement: path.join(__dirname, "fixtures.ts"),
      },
      {
        find: "@tauri-apps/api/app",
        replacement: path.join(__dirname, "fixtures.ts"),
      },
      { find: "@", replacement: path.join(root, "src") },
    ],
  },
  css: { postcss: path.join(root, "postcss.config.js") },
  build: { outDir: path.join(__dirname, "dist"), emptyOutDir: true },
  preview: { host: "127.0.0.1", port: 3011, strictPort: true },
});
