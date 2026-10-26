import { describe, expect, it } from "vitest";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

// TeamoRouter 有两套预设：原 GPT 预设走 OpenAI 兼容协议，Claude 预设走
// Anthropic 原生协议（/v1/messages），两者并存、互不覆盖。
const CLAUDE_MODELS = [
  "claude-opus-5-5",
  "claude-sonnet-5-5",
  "claude-fable-5-1",
];
const API_KEY_URL =
  "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory";

function find<T extends { name: string }>(list: T[], name: string): T {
  const preset = list.find((item) => item.name === name);
  if (!preset) throw new Error(`Missing preset: ${name}`);
  return preset;
}

describe("TeamoRouter Claude presets", () => {
  it("keeps the GPT preset and adds a separate Claude preset in every app", () => {
    for (const list of [
      openclawProviderPresets,
      opencodeProviderPresets,
      piProviderPresets,
      hermesProviderPresets,
    ] as { name: string; partnerPromotionKey?: string }[][]) {
      expect(find(list, "TeamoRouter")).toBeDefined();
      const claude = find(list, "TeamoRouter Claude");
      expect(claude).toMatchObject({
        websiteUrl: "https://teamorouter.cn",
        apiKeyUrl: API_KEY_URL,
        category: "aggregator",
        isPartner: true,
        partnerPromotionKey: "teamorouter",
        icon: "teamorouter",
      });
    }
  });

  it("uses Anthropic Messages for OpenClaw with rebased Opus/Sonnet defaults", () => {
    const preset = find(openclawProviderPresets, "TeamoRouter Claude");
    expect(preset.settingsConfig.api).toBe("anthropic-messages");
    expect(preset.settingsConfig.baseUrl).toBe("https://api.teamorouter.cn");
    expect(
      (preset.settingsConfig.models ?? []).map((model) => model.id),
    ).toEqual(CLAUDE_MODELS);

    const rebased = rebaseOpenClawSuggestedDefaults(
      preset.suggestedDefaults!,
      "my-teamo",
    );
    expect(rebased.model).toEqual({
      primary: "my-teamo/claude-opus-5-5",
      fallbacks: ["my-teamo/claude-sonnet-5-5"],
    });
  });

  it("uses @ai-sdk/anthropic for OpenCode with a /v1 root", () => {
    const preset = find(opencodeProviderPresets, "TeamoRouter Claude");
    expect(preset.settingsConfig.npm).toBe("@ai-sdk/anthropic");
    expect(preset.settingsConfig.options?.baseURL).toBe(
      "https://api.teamorouter.cn/v1",
    );
    expect(Object.keys(preset.settingsConfig.models ?? {})).toEqual(
      CLAUDE_MODELS,
    );
  });

  it("uses a distinct Pi provider key and an Anthropic root", () => {
    const gpt = find(piProviderPresets, "TeamoRouter");
    const preset = find(piProviderPresets, "TeamoRouter Claude");
    expect(preset.providerKey).toBe("cc-switch-teamo-router-claude");
    expect(preset.providerKey).not.toBe(gpt.providerKey);
    expect(preset.settingsConfig.api).toBe("anthropic-messages");
    expect(preset.settingsConfig.baseUrl).toBe("https://api.teamorouter.cn");
    expect(preset.settingsConfig.models.map((model) => model.id)).toEqual(
      CLAUDE_MODELS,
    );
  });

  it("uses anthropic_messages for Hermes under its own provider name", () => {
    const gpt = find(hermesProviderPresets, "TeamoRouter");
    const preset = find(hermesProviderPresets, "TeamoRouter Claude");
    expect(preset.settingsConfig.api_mode).toBe("anthropic_messages");
    expect(preset.settingsConfig.base_url).toBe("https://api.teamorouter.cn");
    expect(preset.settingsConfig.name).not.toBe(gpt.settingsConfig.name);
    expect(preset.suggestedDefaults?.model).toEqual({
      default: "claude-opus-5-5",
      provider: preset.settingsConfig.name,
    });
  });
});
