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
    autoSyncEnabled: false,
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

  it("loads automatic sync as disabled by default", async () => {
    renderPanel();

    expect(
      await screen.findByText("usage.modelsDevAutoSync.title"),
    ).toBeInTheDocument();
    expect(screen.getByText(state.configPath)).toBeInTheDocument();
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(saveModelsDevSyncConfig).not.toHaveBeenCalled();
  });

  it("persists disabling without showing the overwrite warning", async () => {
    const enabledState = {
      ...state,
      config: { ...state.config, autoSyncEnabled: true },
    };
    getModelsDevSyncConfig.mockResolvedValue(enabledState);
    renderPanel();

    fireEvent.click(await screen.findByRole("switch"));
    await waitFor(() =>
      expect(saveModelsDevSyncConfig).toHaveBeenCalledWith({
        ...enabledState.config,
        autoSyncEnabled: false,
      }),
    );
    expect(
      screen.queryByText("usage.modelsDevAutoSync.enableConfirmTitle"),
    ).not.toBeInTheDocument();
  });

  it("warns about price overwrites before enabling automatic sync", async () => {
    renderPanel();

    fireEvent.click(await screen.findByRole("switch"));

    expect(saveModelsDevSyncConfig).not.toHaveBeenCalled();
    expect(
      await screen.findByText("usage.modelsDevAutoSync.enableConfirmTitle"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.modelsDevAutoSync.enableConfirmAction",
      }),
    );

    await waitFor(() =>
      expect(saveModelsDevSyncConfig).toHaveBeenCalledWith({
        ...state.config,
        autoSyncEnabled: true,
      }),
    );
  });

  it("keeps automatic sync disabled when the overwrite warning is cancelled", async () => {
    renderPanel();

    fireEvent.click(await screen.findByRole("switch"));
    fireEvent.click(
      await screen.findByRole("button", { name: "common.cancel" }),
    );

    expect(saveModelsDevSyncConfig).not.toHaveBeenCalled();
    expect(screen.getByRole("switch")).not.toBeChecked();
  });

  it("reloads the automatic sync config after reading the local pricing file", async () => {
    const initialState = {
      ...state,
      config: { ...state.config, autoSyncEnabled: true },
    };
    getModelsDevSyncConfig
      .mockResolvedValueOnce(initialState)
      .mockResolvedValue(state);
    renderPanel();

    fireEvent.click(
      await screen.findByRole("button", {
        name: "usage.modelsDevAutoSync.reloadLocalFile",
      }),
    );

    await waitFor(() =>
      expect(getModelsDevSyncConfig).toHaveBeenCalledTimes(2),
    );
    expect(getModelPricing).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeChecked());
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
