import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { codeInspectorPlugin } from "code-inspector-plugin";

function getPackageName(id: string): string | undefined {
  const marker = "node_modules/";
  const markerIndex = id.lastIndexOf(marker);
  if (markerIndex === -1) return undefined;

  const parts = id.slice(markerIndex + marker.length).split("/");
  if (!parts[0]) return undefined;
  return parts[0].startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0];
}

export default defineConfig(({ command }) => ({
  root: "src",
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
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("/src/i18n/locales/")) {
            return "app-i18n";
          }
          if (id.includes("/src/icons/extracted/")) {
            return "app-provider-icons";
          }
          if (
            id.includes("/src/config/") &&
            (id.includes("ProviderPresets") ||
              id.includes("codexTemplates") ||
              id.includes("codingPlanProviders"))
          ) {
            return "app-provider-presets";
          }
          if (
            id.includes("/src/components/usage/") ||
            id.endsWith("/src/components/UsageFooter.tsx") ||
            id.endsWith("/src/components/UsageScriptModal.tsx")
          ) {
            return "app-usage";
          }
          if (id.includes("/src/components/settings/")) {
            return "app-settings";
          }
          if (id.includes("/src/components/providers/")) {
            return "app-providers";
          }
          if (id.includes("/src/components/sessions/")) {
            return "app-sessions";
          }
          if (id.includes("/src/components/skills/")) {
            return "app-skills";
          }

          const packageName = getPackageName(id);
          if (!packageName) return undefined;

          if (["react", "react-dom", "scheduler"].includes(packageName)) {
            return "vendor-react";
          }
          if (packageName.startsWith("@tauri-apps/")) {
            return "vendor-tauri";
          }
          if (packageName.startsWith("@tanstack/")) {
            return "vendor-query";
          }
          if (
            packageName.startsWith("@radix-ui/") ||
            ["cmdk", "class-variance-authority", "clsx", "sonner"].includes(
              packageName,
            )
          ) {
            return "vendor-ui";
          }
          if (packageName === "lucide-react") {
            return "vendor-icons";
          }
          if (
            packageName === "framer-motion" ||
            packageName.startsWith("motion-")
          ) {
            return "vendor-motion";
          }
          if (
            packageName === "codemirror" ||
            [
              "@codemirror/autocomplete",
              "@codemirror/commands",
              "@codemirror/language",
              "@codemirror/lint",
              "@codemirror/search",
              "@codemirror/state",
              "@codemirror/view",
            ].includes(packageName) ||
            ["crelt", "style-mod", "w3c-keyname"].includes(packageName)
          ) {
            return "vendor-editor-core";
          }
          if (
            packageName.startsWith("@codemirror/lang-") ||
            packageName.startsWith("@lezer/")
          ) {
            return "vendor-editor-language";
          }
          if (packageName.startsWith("@codemirror/theme-")) {
            return "vendor-editor-theme";
          }
          if (packageName === "prettier") {
            if (id.includes("/parser-babel.")) {
              return "vendor-prettier-babel";
            }
            if (id.includes("/plugins/estree.")) {
              return "vendor-prettier-estree";
            }
            return "vendor-prettier";
          }
          if (
            packageName === "recharts" ||
            packageName.startsWith("d3-") ||
            ["decimal.js-light", "victory-vendor"].includes(packageName)
          ) {
            return "vendor-charts";
          }
          if (
            packageName.startsWith("@dnd-kit/") ||
            [
              "@hookform/resolvers",
              "flexsearch",
              "jsonc-parser",
              "react-hook-form",
              "smol-toml",
              "tailwind-merge",
              "zod",
            ].includes(packageName)
          ) {
            return "vendor-tools";
          }

          return "vendor";
        },
      },
    },
  },
  server: {
    port: 3000,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
}));
