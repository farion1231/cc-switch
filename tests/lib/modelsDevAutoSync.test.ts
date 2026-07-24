import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  getModelsDevSyncConfig,
  updateModelPricingBatch,
  recordModelsDevSyncResult,
} = vi.hoisted(() => ({
  getModelsDevSyncConfig: vi.fn(),
  updateModelPricingBatch: vi.fn(),
  recordModelsDevSyncResult: vi.fn(),
}));

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getModelsDevSyncConfig,
    updateModelPricingBatch,
    recordModelsDevSyncResult,
  },
}));

import { syncModelsDevPricing } from "@/lib/modelsDevAutoSync";

const state = {
  configPath: "C:/Users/test/.cc-switch/model-pricing.json",
  config: {
    autoSyncEnabled: true,
    includeCommonModels: true,
    selectedModelKeys: ["relay/custom-model"],
    excludedCommonModelKeys: [],
    lastSyncAt: null,
    lastSyncError: null,
  },
};

describe("syncModelsDevPricing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateModelPricingBatch.mockResolvedValue(2);
    recordModelsDevSyncResult.mockResolvedValue(undefined);
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          openai: {
            models: {
              "gpt-5": {
                name: "GPT-5",
                release_date: "2025-08-01",
                cost: { input: 1, output: 2 },
              },
            },
          },
          relay: {
            models: {
              "custom-model": {
                name: "Custom Model",
                release_date: "2025-07-01",
                cost: { input: 0.5, output: 1 },
              },
            },
          },
        }),
      }),
    );
  });

  it("skips network access when automatic sync is disabled", async () => {
    getModelsDevSyncConfig.mockResolvedValue({
      ...state,
      config: { ...state.config, autoSyncEnabled: false },
    });

    const result = await syncModelsDevPricing();

    expect(result.skipped).toBe(true);
    expect(fetch).not.toHaveBeenCalled();
    expect(updateModelPricingBatch).not.toHaveBeenCalled();
  });

  it("imports common and explicitly selected models in one batch", async () => {
    getModelsDevSyncConfig.mockResolvedValue(state);

    const result = await syncModelsDevPricing();

    expect(updateModelPricingBatch).toHaveBeenCalledTimes(1);
    expect(updateModelPricingBatch).toHaveBeenCalledWith([
      expect.objectContaining({ modelId: "gpt-5", inputCostPerMillion: "1" }),
      expect.objectContaining({
        modelId: "custom-model",
        inputCostPerMillion: "0.5",
      }),
    ]);
    expect(result).toMatchObject({
      skipped: false,
      selected: 2,
      imported: 2,
      changed: 2,
    });
    expect(recordModelsDevSyncResult).toHaveBeenCalledWith(
      expect.any(Number),
      null,
    );
  });

  it("persists the last error without replacing the previous success time", async () => {
    const previous = { ...state, config: { ...state.config, lastSyncAt: 123 } };
    getModelsDevSyncConfig.mockResolvedValue(previous);
    vi.mocked(fetch).mockRejectedValueOnce(new Error("offline"));

    await expect(syncModelsDevPricing()).rejects.toThrow("offline");
    expect(recordModelsDevSyncResult).toHaveBeenCalledWith(null, "offline");
  });
});
