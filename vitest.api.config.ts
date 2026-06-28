import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["tests/api/**/*.test.ts"],
    globals: true,
    testTimeout: 15000,
  },
});
