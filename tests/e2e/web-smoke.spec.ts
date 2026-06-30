import { test, expect } from "@playwright/test";

const STATIC_TOKEN = "e2e-test-token";

test.describe("web mode smoke", () => {
  test("login and navigate to dashboard", async ({ page }) => {
    // 1. Generate a token via the backend API (requires the static AUTH_TOKEN)
    const generateRes = await page.request.post("/api/v1/auth/generate", {
      headers: { Authorization: `Bearer ${STATIC_TOKEN}` },
    });
    expect(generateRes.ok()).toBe(true);
    const generateBody = await generateRes.json();
    expect(generateBody.success).toBe(true);
    const token = generateBody.data as string;
    expect(token).toBeTruthy();

    // 2. Navigate to login page
    await page.goto("/");
    await expect(page.getByText("CC Switch")).toBeVisible();
    await expect(
      page.getByPlaceholder(/Paste your auth token here/i),
    ).toBeVisible();

    // 3. Fill token and submit
    await page.fill('input[id="token"]', token);
    await page.click('button[type="submit"]');

    // 4. Wait for dashboard to load (login form replaced by app content)
    await expect(page.getByRole("banner")).toBeVisible();

    // Dismiss the first-run welcome dialog if it appears (fresh database).
    const gotItButton = page.getByRole("button", { name: /Got it|知道了/i });
    try {
      await gotItButton.waitFor({ state: "visible", timeout: 5000 });
      await gotItButton.click();
      await expect(gotItButton).toBeHidden();
    } catch {
      // No first-run dialog; proceed.
    }

    // 5. Verify the active Claude tab is visible in the AppSwitcher
    const claudeTab = page.getByRole("button", { name: /Claude Code$/i });
    await expect(claudeTab).toBeVisible();

    // 6. Click the Claude tab and verify the empty-state content loads
    await claudeTab.click();
    await expect(
      page.getByRole("heading", {
        name: /No providers added yet|还没有添加任何供应商/i,
      }),
    ).toBeVisible();
  });

  test("invalid token shows error and does not navigate", async ({ page }) => {
    // 1. Navigate to login page
    await page.goto("/");
    await expect(
      page.getByPlaceholder(/Paste your auth token here/i),
    ).toBeVisible();

    // 2. Fill an obviously invalid token and submit
    await page.fill('input[id="token"]', "not-a-valid-token");
    await page.click('button[type="submit"]');

    // 3. Expect error toast to appear and stay on login page
    await expect(page.getByText(/Login failed|Invalid token/i)).toBeVisible();
    await expect(
      page.getByPlaceholder(/Paste your auth token here/i),
    ).toBeVisible();
  });
});
