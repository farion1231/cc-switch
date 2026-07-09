import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useApiKeyState } from "@/components/providers/forms/hooks/useApiKeyState";

describe("useApiKeyState", () => {
  describe("showApiKey", () => {
    it("shows the input whenever a preset is selected (add mode)", () => {
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange: vi.fn(),
          selectedPresetId: "custom",
          category: "custom",
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey("{}", false)).toBe(true);
    });

    it("shows the input in edit mode for custom providers even without an API key field (#5041)", () => {
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange: vi.fn(),
          selectedPresetId: null,
          category: "custom",
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey("{}", true)).toBe(true);
    });

    it("shows the input in edit mode when category is undefined (imported/legacy providers)", () => {
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange: vi.fn(),
          selectedPresetId: null,
          category: undefined,
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey("{}", true)).toBe(true);
    });

    it("hides the input in edit mode for official providers without an API key field", () => {
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange: vi.fn(),
          selectedPresetId: null,
          category: "official",
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey("{}", true)).toBe(false);
    });

    it("shows the input in edit mode for official providers when the config already has the field", () => {
      const config = JSON.stringify({ env: { ANTHROPIC_AUTH_TOKEN: "sk-1" } });
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: config,
          onConfigChange: vi.fn(),
          selectedPresetId: null,
          category: "official",
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey(config, true)).toBe(true);
    });

    it("hides the input in edit mode for cloud providers without an API key field", () => {
      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange: vi.fn(),
          selectedPresetId: null,
          category: "cloud_provider",
          appType: "claude",
        }),
      );

      expect(result.current.showApiKey("{}", true)).toBe(false);
    });
  });

  describe("handleApiKeyChange", () => {
    it("creates the missing API key field in edit mode for custom providers (#5041)", () => {
      let latestConfig = "{}";
      const onConfigChange = vi.fn((config: string) => {
        latestConfig = config;
      });

      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange,
          selectedPresetId: null,
          category: "custom",
          appType: "claude",
        }),
      );

      act(() => {
        result.current.handleApiKeyChange("sk-new-key");
      });

      expect(JSON.parse(latestConfig).env.ANTHROPIC_AUTH_TOKEN).toBe(
        "sk-new-key",
      );
    });

    it("creates the missing API key field in edit mode when category is undefined", () => {
      let latestConfig = "{}";
      const onConfigChange = vi.fn((config: string) => {
        latestConfig = config;
      });

      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange,
          selectedPresetId: null,
          category: undefined,
          appType: "claude",
        }),
      );

      act(() => {
        result.current.handleApiKeyChange("sk-new-key");
      });

      expect(JSON.parse(latestConfig).env.ANTHROPIC_AUTH_TOKEN).toBe(
        "sk-new-key",
      );
    });

    it("respects apiKeyField when creating the missing field", () => {
      let latestConfig = "{}";
      const onConfigChange = vi.fn((config: string) => {
        latestConfig = config;
      });

      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange,
          selectedPresetId: null,
          category: "custom",
          appType: "claude",
          apiKeyField: "ANTHROPIC_API_KEY",
        }),
      );

      act(() => {
        result.current.handleApiKeyChange("sk-new-key");
      });

      const env = JSON.parse(latestConfig).env;
      expect(env.ANTHROPIC_API_KEY).toBe("sk-new-key");
      expect(env.ANTHROPIC_AUTH_TOKEN).toBeUndefined();
    });

    it("does not create the field for official providers", () => {
      const onConfigChange = vi.fn();

      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig: "{}",
          onConfigChange,
          selectedPresetId: null,
          category: "official",
          appType: "claude",
        }),
      );

      act(() => {
        result.current.handleApiKeyChange("sk-new-key");
      });

      expect(onConfigChange).toHaveBeenCalledWith("{}");
    });

    it("updates the existing field for official providers without creating new ones", () => {
      const initialConfig = JSON.stringify({
        env: { ANTHROPIC_API_KEY: "sk-old" },
      });
      let latestConfig = initialConfig;
      const onConfigChange = vi.fn((config: string) => {
        latestConfig = config;
      });

      const { result } = renderHook(() =>
        useApiKeyState({
          initialConfig,
          onConfigChange,
          selectedPresetId: null,
          category: "official",
          appType: "claude",
        }),
      );

      act(() => {
        result.current.handleApiKeyChange("sk-updated");
      });

      const env = JSON.parse(latestConfig).env;
      expect(env.ANTHROPIC_API_KEY).toBe("sk-updated");
      expect(env.ANTHROPIC_AUTH_TOKEN).toBeUndefined();
    });
  });
});
