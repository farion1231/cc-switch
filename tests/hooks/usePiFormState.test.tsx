import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { usePiFormState } from "@/components/providers/forms/hooks/usePiFormState";

const PI_CONFIG_A = JSON.stringify({
  baseUrl: "https://a.example/v1",
  api: "openai-completions",
  models: [{ id: "model-a", name: "Model A" }],
  defaultModel: "model-a",
});

const PI_CONFIG_B = JSON.stringify({
  baseURL: "https://b.example/v1",
  apiKey: "$PI_KEY",
  api: "google-generative-ai",
  models: ["gemma-4"],
  defaultModel: "gemma-4",
});

describe("usePiFormState", () => {
  it("updates the selected default model even while the JSON editor is invalid", () => {
    const validConfig = JSON.stringify({
      baseUrl: "https://api.example.com/v1",
      apiKey: "sk-test",
      api: "openai-completions",
      models: [
        { id: "old-model", name: "Old Model" },
        { id: "next-model", name: "Next Model" },
      ],
      defaultModel: "old-model",
    });
    const onSettingsConfigChange = vi.fn();

    const { result, rerender } = renderHook(
      ({ settingsConfig }) =>
        usePiFormState({
          appId: "pi",
          settingsConfig,
          onSettingsConfigChange,
        }),
      { initialProps: { settingsConfig: validConfig } },
    );

    rerender({ settingsConfig: "{ invalid json" });
    act(() => {
      result.current.handlePiModelsChange([
        { id: "next-model", name: "Next Model" },
      ]);
    });

    expect(result.current.piDefaultModel).toBe("next-model");
    expect(onSettingsConfigChange).not.toHaveBeenCalled();
  });

  it("hydrates structured fields when raw JSON changes", () => {
    const onSettingsConfigChange = vi.fn();
    const { result, rerender } = renderHook(
      ({ settingsConfig }) =>
        usePiFormState({
          appId: "pi",
          settingsConfig,
          onSettingsConfigChange,
        }),
      { initialProps: { settingsConfig: PI_CONFIG_A } },
    );

    rerender({ settingsConfig: PI_CONFIG_B });

    expect(result.current.piBaseUrl).toBe("https://b.example/v1");
    expect(result.current.piApiKey).toBe("$PI_KEY");
    expect(result.current.piApi).toBe("google-generative-ai");
    expect(result.current.piModels).toEqual([
      { id: "gemma-4", name: "gemma-4" },
    ]);
    expect(result.current.piDefaultModel).toBe("gemma-4");
  });

  it("preserves unknown Pi fields during structured edits", () => {
    const onSettingsConfigChange = vi.fn();
    const { result } = renderHook(() =>
      usePiFormState({
        appId: "pi",
        settingsConfig: JSON.stringify({
          baseUrl: "https://api.example.com/v1",
          api: "openai-completions",
          models: [{ id: "model-1", name: "Model 1" }],
          headers: { "x-route": "$PI_ROUTE" },
          futurePiField: { enabled: true },
        }),
        onSettingsConfigChange,
      }),
    );

    act(() => {
      result.current.handlePiApiChange("openai-responses");
    });

    const updated = JSON.parse(onSettingsConfigChange.mock.calls.at(-1)?.[0]);
    expect(updated.api).toBe("openai-responses");
    expect(updated.headers).toEqual({ "x-route": "$PI_ROUTE" });
    expect(updated.futurePiField).toEqual({ enabled: true });
  });
});
