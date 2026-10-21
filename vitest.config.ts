import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    include: ["{src,tests}/**/*.{test,spec}.{ts,tsx,js,jsx}"],
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    // The default 5s budget assumes an unloaded machine. The suite runs 140
    // files in parallel, and the interaction-heavy provider-form cases
    // (PiProviderForm drives ~10 awaited user-event sequences per test) spend
    // their whole budget waiting on a saturated CPU: 0.6-1.2s in isolation, but
    // `Test timed out in 5000ms` under full-suite load. 20s keeps the hang
    // signal while making the verdict about the code, not the box's load.
    testTimeout: 20000,
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
