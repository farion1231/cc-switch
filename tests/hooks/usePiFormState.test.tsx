import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { usePiFormState } from "@/components/providers/forms/hooks/usePiFormState";

describe("usePiFormState", () => {
  it("updates the selected default model even while the JSON editor is invalid", () => {
    const initialData = {
      settingsConfig: {
        baseUrl: "https://api.example.com/v1",
        apiKey: "sk-test",
        api: "openai-chat",
        models: [
          { id: "old-model", name: "Old Model" },
          { id: "next-model", name: "Next Model" },
        ],
        defaultModel: "old-model",
      },
    };
    const onSettingsConfigChange = vi.fn();
    const getSettingsConfig = () => "{ invalid json";

    const { result } = renderHook(() =>
      usePiFormState({
        initialData,
        appId: "pi",
        onSettingsConfigChange,
        getSettingsConfig,
      }),
    );

    act(() => {
      result.current.handlePiModelsChange([
        { id: "next-model", name: "Next Model" },
      ]);
    });

    expect(result.current.piDefaultModel).toBe("next-model");
    expect(onSettingsConfigChange).not.toHaveBeenCalled();
  });
});
