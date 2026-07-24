import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  getModelsDevSyncConfig,
  saveModelsDevSyncConfig,
  getModelPricing,
  openAppConfigFolder,
  syncModelsDevPricing,
} = vi.hoisted(() => ({
  getModelsDevSyncConfig: vi.fn(),
  saveModelsDevSyncConfig: vi.fn(),
  getModelPricing: vi.fn(),
  openAppConfigFolder: vi.fn(),
  syncModelsDevPricing: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) =>
      options?.count == null ? key : `${key}:${options.count}`,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getModelsDevSyncConfig,
    saveModelsDevSyncConfig,
    getModelPricing,
  },
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openAppConfigFolder },
}));

vi.mock("@/lib/modelsDevAutoSync", () => ({
  MODELS_DEV_SYNC_CONFIG_QUERY_KEY: ["models-dev-sync-config"],
  syncModelsDevPricing,
}));

import { ModelsDevAutoSyncPanel } from "@/components/usage/ModelsDevAutoSyncPanel";

const state = {
  configPath: "C:/Users/test/.cc-switch/model-pricing.json",
  config: {
    autoSyncEnabled: true,
    includeCommonModels: true,
    selectedModelKeys: [],
    excludedCommonModelKeys: [],
    lastSyncAt: null,
    lastSyncError: null,
  },
};

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ModelsDevAutoSyncPanel />
    </QueryClientProvider>,
  );
}

describe("ModelsDevAutoSyncPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getModelsDevSyncConfig.mockResolvedValue(state);
    saveModelsDevSyncConfig.mockResolvedValue(undefined);
    getModelPricing.mockResolvedValue([]);
    openAppConfigFolder.mockResolvedValue(undefined);
    syncModelsDevPricing.mockResolvedValue({
      skipped: false,
      selected: 2,
      imported: 2,
      changed: 1,
      syncedAt: Date.now(),
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          openai: {
            name: "OpenAI",
            models: {
              "gpt-5": {
                name: "GPT-5",
                release_date: "2025-08-01",
                cost: { input: 1, output: 2 },
              },
            },
          },
          deepseek: {
            name: "DeepSeek",
            models: {
              "deepseek-chat": {
                name: "DeepSeek Chat",
                release_date: "2025-12-01",
                cost: { input: 0.3, output: 1.2 },
              },
            },
          },
        }),
      }),
    );
  });

  it("loads the default-on setting and persists toggle changes", async () => {
    renderPanel();

    expect(
      await screen.findByText("usage.modelsDevAutoSync.title"),
    ).toBeInTheDocument();
    expect(screen.getByText(state.configPath)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(saveModelsDevSyncConfig).toHaveBeenCalledWith({
        ...state.config,
        autoSyncEnabled: false,
      }),
    );
  });

  it("opens the searchable multi-select dialog with common models selected", async () => {
    renderPanel();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "usage.modelsDevAutoSync.configure",
      }),
    );

    expect(
      await screen.findByText("usage.modelsDevAutoSync.configureTitle"),
    ).toBeInTheDocument();
    expect(await screen.findByText("GPT-5")).toBeInTheDocument();
    expect(screen.getByText("DeepSeek Chat")).toBeInTheDocument();
    expect(
      screen.getByText("usage.modelsDevAutoSync.selectedCount:2"),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("usage.modelsDevAutoSync.commonBadge"),
    ).toHaveLength(2);
  });
});
