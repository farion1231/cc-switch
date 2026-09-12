import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const rootDir = path.resolve(__dirname);
const realRootDir = fs.realpathSync(rootDir);

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    fs: {
      allow: [rootDir, realRootDir],
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    // Cap fork concurrency: 16 logical cores oversubscribed 10 physical ones,
    // causing timeout flakes under full parallel runs.
    poolOptions: {
      forks: {
        minForks: 1,
        maxForks: 6,
      },
    },
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});