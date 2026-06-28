import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

async function importWebClient() {
  vi.resetModules();
  return import("@/lib/api/web-client");
}

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });

describe("web-client", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("auth token lifecycle", () => {
    it("sets token in memory and localStorage", async () => {
      const { setAuthToken, getAuthToken } = await importWebClient();
      setAuthToken("my-token");
      expect(getAuthToken()).toBe("my-token");
      expect(localStorage.getItem("cc_switch_token")).toBe("my-token");
    });

    it("clears token from memory and localStorage", async () => {
      const { setAuthToken, clearAuthToken, getAuthToken } =
        await importWebClient();
      setAuthToken("my-token");
      clearAuthToken();
      expect(getAuthToken()).toBeNull();
      expect(localStorage.getItem("cc_switch_token")).toBeNull();
    });

    it("restores token from localStorage on import", async () => {
      localStorage.setItem("cc_switch_token", "stored-token");
      const { getAuthToken } = await importWebClient();
      expect(getAuthToken()).toBe("stored-token");
    });
  });

  describe("fetchWithAuth", () => {
    it("injects Bearer token when authToken is set", async () => {
      const { setAuthToken, get } = await importWebClient();
      setAuthToken("bearer-token");

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await get("/test");

      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/test",
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: "Bearer bearer-token",
          }),
        }),
      );
    });

    it("omits Authorization header when no token is set", async () => {
      const { get } = await importWebClient();

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await get("/test");

      const callArgs = fetchMock.mock.calls[0];
      const headers = (callArgs[1] as RequestInit)?.headers as Record<
        string,
        string
      >;
      expect(headers).not.toHaveProperty("Authorization");
    });

    it("clears token and dispatches auth:expired on 401", async () => {
      const { setAuthToken, get, getAuthToken } = await importWebClient();
      setAuthToken("expired-token");

      const dispatchSpy = vi.spyOn(window, "dispatchEvent");

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(
          JSON.stringify({ success: false, error: "Unauthorized" }),
          {
            status: 401,
          },
        ),
      );

      await expect(get("/test")).rejects.toThrow("Unauthorized");
      expect(getAuthToken()).toBeNull();
      expect(localStorage.getItem("cc_switch_token")).toBeNull();
      expect(dispatchSpy).toHaveBeenCalledWith(
        expect.objectContaining({ type: "auth:expired" }),
      );
    });

    it("still dispatches auth:expired on 401 when already on /login", async () => {
      const { setAuthToken, get } = await importWebClient();
      setAuthToken("expired-token");

      const dispatchSpy = vi.spyOn(window, "dispatchEvent");

      const originalLocation = window.location;
      Object.defineProperty(window, "location", {
        writable: true,
        value: { ...originalLocation, pathname: "/login", href: "/login" },
      });

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(
          JSON.stringify({ success: false, error: "Unauthorized" }),
          {
            status: 401,
          },
        ),
      );

      try {
        await expect(get("/test")).rejects.toThrow("Unauthorized");
        expect(dispatchSpy).toHaveBeenCalledWith(
          expect.objectContaining({ type: "auth:expired" }),
        );
      } finally {
        Object.defineProperty(window, "location", {
          writable: true,
          value: originalLocation,
        });
      }
    });
  });

  describe("parseApiEnvelope", () => {
    it("returns data for a successful envelope", async () => {
      const { get } = await importWebClient();

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        jsonResponse({ success: true, data: { id: 1 } }),
      );

      const result = await get("/test");
      expect(result).toEqual({ id: 1 });
    });

    it("throws with error message when envelope success is false", async () => {
      const { get } = await importWebClient();

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        jsonResponse({ success: false, error: "Something went wrong" }),
      );

      await expect(get("/test")).rejects.toThrow("Something went wrong");
    });

    it("throws with HTTP status for empty response body", async () => {
      const { get } = await importWebClient();

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response("", { status: 500, statusText: "Internal Server Error" }),
      );

      await expect(get("/test")).rejects.toThrow(
        "HTTP 500 Internal Server Error",
      );
    });

    it("throws with HTTP status for invalid JSON", async () => {
      const { get } = await importWebClient();

      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response("not json", {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      );

      await expect(get("/test")).rejects.toThrow("HTTP 200");
    });
  });

  describe("HTTP methods", () => {
    it("get uses GET method", async () => {
      const { get } = await importWebClient();

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await get("/test");

      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/test",
        expect.objectContaining({ headers: expect.any(Object) }),
      );

      const callArgs = fetchMock.mock.calls[0];
      const init = callArgs[1] as RequestInit;
      expect(init.method).toBe("GET");
    });

    it("post uses POST method and serializes body", async () => {
      const { post } = await importWebClient();

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await post("/test", { name: "value" });

      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/test",
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({ name: "value" }),
        }),
      );
    });

    it("put uses PUT method and serializes body", async () => {
      const { put } = await importWebClient();

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await put("/test", { name: "value" });

      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/test",
        expect.objectContaining({
          method: "PUT",
          body: JSON.stringify({ name: "value" }),
        }),
      );
    });

    it("del uses DELETE method", async () => {
      const { del } = await importWebClient();

      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(jsonResponse({ success: true, data: "ok" }));

      await del("/test");

      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/test",
        expect.objectContaining({ method: "DELETE" }),
      );
    });
  });
});
