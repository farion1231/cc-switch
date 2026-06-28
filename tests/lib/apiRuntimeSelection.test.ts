// NOTE: this test mutates global window.__TAURI__; it must run in an
// isolated process (single file) so that module re-imports see a clean
// environment.  Do NOT import this file into a shared test suite.
import { afterEach, describe, expect, it, vi } from "vitest";

const jsonResponse = (data: unknown) =>
  new Response(JSON.stringify({ success: true, data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

async function importApiInWebMode() {
  // @ts-expect-error - test controls the injected Tauri globals
  delete window.__TAURI__;
  // @ts-expect-error - test controls the injected Tauri globals
  delete window.__TAURI_INTERNALS__;
  vi.resetModules();
  return import("@/lib/api");
}

describe("runtime API selection", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    // @ts-expect-error - restore the default test environment for other tests
    window.__TAURI__ = {};
  });

  it("uses web proxy endpoints when Tauri globals are absent", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ running: false, port: 0 }));
    const { proxyApi } = await importApiInWebMode();

    await proxyApi.getProxyStatus();

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/proxy/status",
      expect.any(Object),
    );
  });

  it("uses web modules for prompts skills and sessions in browser mode", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(
      async (input) => {
        const url = String(input);
        if (url === "/api/v1/prompts?app=claude") {
          return jsonResponse([]);
        }
        if (url === "/api/v1/skills/installed") {
          return jsonResponse([]);
        }
        if (url === "/api/v1/sessions") {
          return jsonResponse([]);
        }
        throw new Error(`Unexpected request: ${url}`);
      },
    );
    const { promptsApi, skillsApi, sessionsApi } = await importApiInWebMode();

    await promptsApi.getPrompts("claude");
    await skillsApi.getInstalled();
    await sessionsApi.list();

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/prompts?app=claude",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/skills/installed",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/sessions",
      expect.any(Object),
    );
  });
});