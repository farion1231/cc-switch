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

import {
  MODELS_DEV_STARTUP_SYNC_INTERVAL_MS,
  syncModelsDevPricing,
} from "@/lib/modelsDevAutoSync";

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

  it("skips startup network access when pricing synced within the interval", async () => {
    const lastSyncAt = Date.now() - MODELS_DEV_STARTUP_SYNC_INTERVAL_MS + 1;
    getModelsDevSyncConfig.mockResolvedValue({
      ...state,
      config: { ...state.config, lastSyncAt },
    });

    const result = await syncModelsDevPricing();

    expect(result).toEqual({
      skipped: true,
      selected: 0,
      imported: 0,
      changed: 0,
      syncedAt: lastSyncAt,
    });
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
    const fetchOptions = vi.mocked(fetch).mock.calls[0]?.[1];
    expect(fetchOptions).toEqual({ signal: expect.any(AbortSignal) });
    expect(fetchOptions).not.toHaveProperty("cache");
  });

  it("stops a startup sync when automatic sync is disabled during download", async () => {
    getModelsDevSyncConfig.mockResolvedValueOnce(state).mockResolvedValueOnce({
      ...state,
      config: { ...state.config, autoSyncEnabled: false, lastSyncAt: 123 },
    });

    const result = await syncModelsDevPricing();

    expect(result).toEqual({
      skipped: true,
      selected: 0,
      imported: 0,
      changed: 0,
      syncedAt: 123,
    });
    expect(updateModelPricingBatch).not.toHaveBeenCalled();
    expect(recordModelsDevSyncResult).not.toHaveBeenCalled();
  });

  it("uses the latest model selection after the download completes", async () => {
    getModelsDevSyncConfig.mockResolvedValueOnce(state).mockResolvedValueOnce({
      ...state,
      config: {
        ...state.config,
        includeCommonModels: false,
        selectedModelKeys: ["relay/custom-model"],
      },
    });

    await syncModelsDevPricing();

    expect(updateModelPricingBatch).toHaveBeenCalledWith([
      expect.objectContaining({ modelId: "custom-model" }),
    ]);
  });

  it("uses the latest selection for a forced sync even when automatic sync is disabled", async () => {
    getModelsDevSyncConfig.mockResolvedValueOnce({
      ...state,
      config: {
        ...state.config,
        autoSyncEnabled: false,
        includeCommonModels: false,
        selectedModelKeys: ["relay/custom-model"],
        lastSyncAt: Date.now(),
      },
    });

    const result = await syncModelsDevPricing(state, true);

    expect(result.skipped).toBe(false);
    expect(updateModelPricingBatch).toHaveBeenCalledWith([
      expect.objectContaining({ modelId: "custom-model" }),
    ]);
  });

  it("persists the last error without replacing the previous success time", async () => {
    const previous = { ...state, config: { ...state.config, lastSyncAt: 123 } };
    getModelsDevSyncConfig.mockResolvedValue(previous);
    vi.mocked(fetch).mockRejectedValueOnce(new Error("offline"));

    await expect(syncModelsDevPricing()).rejects.toThrow("offline");
    expect(recordModelsDevSyncResult).toHaveBeenCalledWith(null, "offline");
  });
});
