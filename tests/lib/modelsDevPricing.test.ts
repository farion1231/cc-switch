import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import { fetchModelsDevPricing } from "@/lib/modelsDevPricing";

const fixture = {
  openai: {
    models: {
      "gpt-5": {
        name: "GPT-5",
        release_date: "2025-08-01",
        cost: { input: 1, output: 2 },
      },
    },
  },
};

describe("fetchModelsDevPricing", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("fetches via the backend command and parses the JSON payload", async () => {
    invokeMock.mockResolvedValueOnce(JSON.stringify(fixture));

    const result = await fetchModelsDevPricing();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("fetch_models_dev_pricing");
    expect(result).toEqual(fixture);
  });

  it("propagates backend errors", async () => {
    invokeMock.mockRejectedValueOnce(new Error("HTTP 503"));

    await expect(fetchModelsDevPricing()).rejects.toThrow("HTTP 503");
  });
});
