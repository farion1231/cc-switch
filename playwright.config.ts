import { defineConfig, devices } from "@playwright/test";
export default defineConfig({
  testDir: "./e2e",
  testMatch: "tandem-ledger.spec.ts",
  outputDir: "/tmp/tandem-task7-playwright-results",
  workers: 1,
  fullyParallel: false,
  use: {
    baseURL: "http://127.0.0.1:3000/tandem-demo.html",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "pnpm run dev:renderer -- --host 127.0.0.1",
    port: 3000,
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
