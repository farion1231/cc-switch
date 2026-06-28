import { afterEach, describe, expect, it, vi } from "vitest";
import { proxyApi } from "@/lib/api/web/proxy";

const jsonResponse = (data: unknown) =>
  new Response(JSON.stringify({ success: true, data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

describe("web proxy API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("calls the stop-with-restore endpoint", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(true));

    await proxyApi.stopProxyWithRestore();

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/proxy/stop-with-restore",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("calls the provider hot-switch endpoint", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(true));

    await proxyApi.switchProxyProvider("claude", "anthropic-router");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/proxy/switch",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ appType: "claude", providerId: "anthropic-router" }),
      }),
    );
  });

  it("loads and saves default cost multiplier through web endpoints", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      if (url === "/api/v1/proxy/default-cost-multiplier?app=claude") {
        return jsonResponse("1.25");
      }
      throw new Error(`Unexpected request: ${url} ${init?.method ?? "GET"}`);
    });

    await expect(proxyApi.getDefaultCostMultiplier("claude")).resolves.toBe("1.25");
    await proxyApi.setDefaultCostMultiplier("claude", "0.75");

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "/api/v1/proxy/default-cost-multiplier?app=claude",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/proxy/default-cost-multiplier?app=claude",
      expect.objectContaining({
        method: "PUT",
        body: JSON.stringify({ value: "0.75" }),
      }),
    );
  });

  it("loads and saves pricing model source through web endpoints", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = String(input);
      if (url === "/api/v1/proxy/pricing-model-source?app=codex") {
        return jsonResponse("response");
      }
      throw new Error(`Unexpected request: ${url}`);
    });

    await expect(proxyApi.getPricingModelSource("codex")).resolves.toBe("response");
    await proxyApi.setPricingModelSource("codex", "request");

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "/api/v1/proxy/pricing-model-source?app=codex",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/proxy/pricing-model-source?app=codex",
      expect.objectContaining({
        method: "PUT",
        body: JSON.stringify({ value: "request" }),
      }),
    );
  });
});