import { afterEach, describe, expect, it, vi } from "vitest";
import { authApi } from "@/lib/api/auth";
import { authApi as webAuthApi } from "@/lib/api/web/auth";

const jsonResponse = (data: unknown) =>
  new Response(JSON.stringify({ success: true, data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

describe("webAuthApi", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("logs in by posting the auth token and returns the JWT envelope", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ token: "jwt-token-from-server" }));

    const result = await webAuthApi.login("user-auth-token");

    expect(result).toEqual({ token: "jwt-token-from-server" });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/auth/login",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ token: "user-auth-token" }),
      }),
    );
  });

  it("throws when the login endpoint returns an error", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({ success: false, error: "Invalid auth token" }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );

    await expect(webAuthApi.login("wrong-token")).rejects.toThrow(
      "Invalid auth token",
    );
  });
});

describe("authApi", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("exposes login and delegates to the web auth endpoint", async () => {
    // Tests run with Tauri globals enabled by default; login is web-only.
    // @ts-ignore - Tauri global
    const originalTauri = window.__TAURI__;
    // @ts-ignore
    delete window.__TAURI__;

    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ token: "jwt-token-from-server" }),
    );

    try {
      const result = await authApi.login("user-auth-token");
      expect(result).toBe("jwt-token-from-server");
    } finally {
      // @ts-ignore
      window.__TAURI__ = originalTauri;
    }
  });
});
