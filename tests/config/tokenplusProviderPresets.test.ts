import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { renderHook } from "@testing-library/react";
import { useApiKeyLink } from "@/components/providers/forms/hooks/useApiKeyLink";

describe("TokenPlus", () => {
  it("offers the four preferred models through the Codex chat router", () => {
    const preset = codexProviderPresets.find((p) => p.name === "TokenPlus")!;
    expect(preset.config).not.toContain("model_reasoning_effort");
    expect(preset.apiFormat).toBe("openai_chat");
    expect(preset.modelCatalog?.map((m) => m.model)).toEqual([
      "deepseek/deepseek-v4.1-flash-1017",
      "z-ai/glm-5.3-flash",
      "moonshot-ai/kimi-k3.1-pro",
      "minimax/m3.1-flash",
    ]);
    expect(
      Object.keys(
        opencodeProviderPresets.find((p) => p.name === "TokenPlus")!
          .settingsConfig.models,
      ),
    ).toHaveLength(4);
  });
  it("recognizes an existing TokenPlus configuration without preset metadata", () => {
    const { result } = renderHook(() =>
      useApiKeyLink({
        appId: "opencode",
        category: "aggregator",
        selectedPresetId: null,
        presetEntries: opencodeProviderPresets.map((preset, index) => ({
          id: `opencode-${index}`,
          preset,
        })),
        formWebsiteUrl: "https://www.tokenplus.cn",
        formBaseUrl: "https://api.tokenplus.cn/v1/",
      }),
    );
    expect(result.current.partnerPromotionKey).toBe("tokenplus");
  });
});
