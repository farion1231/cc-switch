import path from "node:path";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf8"),
);

/**
 * Web 构建目标（零上游源码改动）。
 *
 * 经 `resolve.alias` 把 `@tauri-apps/api/*` 与 `@tauri-apps/plugin-*` 指向
 * `src/web/shim-*.ts`。同一份 `src/` 源码编两个 target：
 *  - 桌面：`vite`（原 config，走真实 Tauri）
 *  - web：  `vite --config vite.web.config.ts`（走 shim → REST）
 *
 * 不挂 `@tauri-apps/plugin-vite`（不注入 Tauri runtime）。
 */
export default defineConfig({
  root: path.resolve(__dirname, "src/web"),
  plugins: [react()],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    port: 3001,
    strictPort: true,
    proxy: {
      // web shell 的 fetch("/control/v1/...") → 二进制 REST 控制面（:8787）
      "/control": { target: "http://127.0.0.1:8787", changeOrigin: true },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@tauri-apps/api/core": path.resolve(__dirname, "./src/web/shim-core.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "./src/web/shim-event.ts"),
      "@tauri-apps/api/window": path.resolve(__dirname, "./src/web/shim-window.ts"),
      "@tauri-apps/api/app": path.resolve(__dirname, "./src/web/shim-app.ts"),
      "@tauri-apps/api/path": path.resolve(__dirname, "./src/web/shim-path.ts"),
      "@tauri-apps/plugin-process": path.resolve(__dirname, "./src/web/shim-process.ts"),
      "@tauri-apps/plugin-log": path.resolve(__dirname, "./src/web/shim-log.ts"),
      "@tauri-apps/plugin-dialog": path.resolve(__dirname, "./src/web/shim-dialog.ts"),
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  define: {
    "import.meta.env.VITE_APP_VERSION": JSON.stringify(pkg.version),
  },
});
