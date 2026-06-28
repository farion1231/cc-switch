import { test, expect } from "@playwright/test";

test.describe("web mode smoke", () => {
  test("login and navigate to dashboard", async ({ page }) => {
    // 1. Generate a token via the backend API
    const generateRes = await page.request.post("/api/v1/auth/generate", {});
    expect(generateRes.ok()).toBe(true);
    const generateBody = await generateRes.json();
    expect(generateBody.success).toBe(true);
    const token = generateBody.data as string;
    expect(token).toBeTruthy();

    // 2. Navigate to login page
    await page.goto("/");
    await expect(page.getByText("CC Switch")).toBeVisible();
    await expect(page.getByPlaceholder(/Paste your admin token here/i)).toBeVisible();

    // 3. Fill token and submit
    await page.fill('input[id="token"]', token);
    await page.click('button[type="submit"]');

    // 4. Wait for dashboard to load (login form replaced by app content)
    await expect(page.getByRole("banner")).toBeVisible();

    // 5. Verify AppSwitcher tabs are visible
    await expect(page.getByRole("button", { name: "Claude Claude" })).toBeVisible();
    await expect(page.getByRole("button", { name: "OpenAI Codex" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Gemini Gemini" })).toBeVisible();

    // 6. Wait for toast to dismiss, then click Claude tab and verify content loads
    await expect(page.getByRole("button", { name: "Claude Claude" })).toBeEnabled();
    await page.getByRole("button", { name: "Claude Claude" }).click();
    await expect(page.getByRole("heading", { name: "No providers added yet" })).toBeVisible();
  });

  test("invalid token shows error and does not navigate", async ({ page }) => {
    // 1. Navigate to login page
    await page.goto("/");
    await expect(page.getByPlaceholder(/Paste your admin token here/i)).toBeVisible();

    // 2. Fill an obviously invalid token and submit
    await page.fill('input[id="token"]', "not-a-valid-token");
    await page.click('button[type="submit"]');

    // 3. Expect error toast to appear and stay on login page
    await expect(page.getByText(/Login failed|Invalid token/i)).toBeVisible();
    await expect(page.getByPlaceholder(/Paste your admin token here/i)).toBeVisible();
  });
});
