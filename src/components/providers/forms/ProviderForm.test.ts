import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { normalizeCodexCatalogModelsForSave } from "./ProviderForm";
import { useCodexConfigState } from "./hooks/useCodexConfigState";

describe("normalizeCodexCatalogModelsForSave", () => {
  it("preserves managed model reasoning capabilities", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: " gpt-5-6-luna ",
          displayName: " 5.6 Luna ",
          contextWindow: "272,000",
          reasoningEfforts: ["none", "low", "medium", "high", "xhigh", "max"],
          defaultReasoningEffort: " high ",
        },
      ]),
    ).toEqual([
      {
        model: "gpt-5-6-luna",
        displayName: "5.6 Luna",
        contextWindow: 272000,
        reasoningEfforts: ["none", "low", "medium", "high", "xhigh", "max"],
        defaultReasoningEffort: "high",
      },
    ]);
  });
});

describe("useCodexConfigState", () => {
  it("backfills reasoning capabilities for existing managed Kiro models", async () => {
    const initialData = {
      meta: { providerType: "kiro" },
      settingsConfig: {
        auth: {},
        config: 'model = "gpt-5-6-luna"',
        modelCatalog: {
          models: [{ model: "gpt-5-6-luna" }],
        },
      },
    };
    const { result } = renderHook(() =>
      useCodexConfigState({
        initialData,
      }),
    );

    await waitFor(() =>
      expect(result.current.codexCatalogModels).toEqual([
        {
          model: "gpt-5-6-luna",
          displayName: "",
          contextWindow: "",
          reasoningEfforts: ["none", "low", "medium", "high", "xhigh", "max"],
          defaultReasoningEffort: "high",
        },
      ]),
    );
  });
});
