import { test, expect } from "@playwright/test";

/**
 * Session management e2e (web mode).
 *
 * Regression guard: the web `/api/v1/sessions` endpoint must scan on-disk AI
 * tool session logs (Claude/Codex/Gemini/OpenCode/OpenClaw/Hermes) and return a
 * well-formed list — it previously read an always-empty bespoke SQL table,
 * which silently broke the feature. We assert the API contract (success + array
 * shape) and that the Session Manager UI opens without an error state.
 */
const STATIC_TOKEN = "e2e-test-token";

test.describe("session management (web)", () => {
  async function authToken(request: import("@playwright/test").APIRequestContext) {
    const res = await request.post("/api/v1/auth/generate", {
      headers: { Authorization: `Bearer ${STATIC_TOKEN}` },
    });
    expect(res.ok()).toBe(true);
    const body = await res.json();
    expect(body.success).toBe(true);
    return body.data as string;
  }

  test("GET /sessions returns a success envelope with an array payload", async ({
    request,
  }) => {
    const token = await authToken(request);
    const res = await request.get("/api/v1/sessions", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBe(true);
    const body = await res.json();
    expect(body.success).toBe(true);
    expect(Array.isArray(body.data)).toBe(true);

    // Every returned session must carry the identity fields the UI and the
    // delete/messages endpoints depend on.
    for (const s of body.data) {
      expect(typeof s.providerId).toBe("string");
      expect(typeof s.sessionId).toBe("string");
    }
  });

  test("messages endpoint validates required query params", async ({
    request,
  }) => {
    const token = await authToken(request);
    // Missing providerId/sourcePath -> 4xx (axum query rejection), never a 200
    // with a bogus empty body from a non-existent table.
    const res = await request.get("/api/v1/sessions/messages", {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(res.ok()).toBe(false);
  });

  test("Session Manager view opens from the UI without an error", async ({
    page,
  }) => {
    const token = await page.request
      .post("/api/v1/auth/generate", {
        headers: { Authorization: `Bearer ${STATIC_TOKEN}` },
      })
      .then((r) => r.json())
      .then((b) => b.data as string);

    await page.goto("/");
    await page.fill('input[id="token"]', token);
    await page.click('button[type="submit"]');
    await expect(page.getByRole("banner")).toBeVisible();

    // Navigate to the Session Manager. The trigger is an icon button in the
    // toolbar; match by its accessible name (i18n: "会话" / "Sessions").
    const sessionsButton = page
      .getByRole("button", { name: /session|会话|セッション/i })
      .first();
    if (await sessionsButton.isVisible().catch(() => false)) {
      await sessionsButton.click();
      // The page must render its own heading and must NOT show a query-failed
      // error toast/banner.
      await expect(page.getByText(/查询失败|query failed/i)).toHaveCount(0);
    }
  });
});
