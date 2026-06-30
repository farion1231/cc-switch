import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://localhost:13002",
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "AUTH_TOKEN=e2e-test-token CC_SWITCH_WEB_PORT=13002 CC_SWITCH_DB_PATH=/tmp/cc-switch-e2e.db pnpm headless:debug:web",
    url: "http://localhost:13002/health",
    timeout: 60 * 1000,
    reuseExistingServer: !process.env.CI,
  },
});
