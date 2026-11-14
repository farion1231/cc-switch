import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useGeminiConfigState } from "@/components/providers/forms/hooks/useGeminiConfigState";

const initialData = {
  settingsConfig: {
    env: { GEMINI_MODEL: "legacy-model", GEMINI_API_KEY: "fixture" },
    config: {
      model: { name: "native-model", temperature: 0.5 },
      theme: "keep",
    },
  },
};

describe("Gemini and Antigravity model configuration", () => {
  it("displays and updates the native agy model, preserving unrelated fields", () => {
    const { result } = renderHook(() =>
      useGeminiConfigState({ initialData, preferConfigModel: true }),
    );
    expect(result.current.geminiModel).toBe("native-model");
    act(() => result.current.handleGeminiModelChange("new-model"));
    expect(result.current.geminiModel).toBe("new-model");
    expect(JSON.parse(result.current.geminiConfig)).toEqual({
      model: { name: "new-model", temperature: 0.5 },
      theme: "keep",
    });
    expect(result.current.geminiEnv).toContain("GEMINI_MODEL=new-model");
    act(() => result.current.handleGeminiModelChange(""));
    expect(result.current.geminiModel).toBe("");
    expect(JSON.parse(result.current.geminiConfig).model.name).toBe("");
  });

  it("keeps the existing Gemini CLI env model behavior", () => {
    const { result } = renderHook(() => useGeminiConfigState({ initialData }));
    expect(result.current.geminiModel).toBe("legacy-model");
    act(() => result.current.handleGeminiModelChange("new-model"));
    expect(result.current.geminiModel).toBe("new-model");
    expect(JSON.parse(result.current.geminiConfig).model.name).toBe(
      "native-model",
    );
  });

  it("loads legacy agy env models and follows valid JSON model edits", () => {
    const legacy = {
      settingsConfig: {
        env: { GEMINI_MODEL: "legacy-model" },
        config: { model: { name: null } },
      },
    };
    const { result } = renderHook(() =>
      useGeminiConfigState({ initialData: legacy, preferConfigModel: true }),
    );
    expect(result.current.geminiModel).toBe("legacy-model");
    act(() =>
      result.current.handleGeminiConfigChange(
        '{"model":{"name":"json-model"}}',
      ),
    );
    expect(result.current.geminiModel).toBe("json-model");
    act(() => result.current.handleGeminiConfigChange('{"model":'));
    expect(result.current.geminiModel).toBe("json-model");
    expect(result.current.configError).not.toBe("");
  });
});
