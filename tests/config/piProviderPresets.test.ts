import { describe, expect, it } from "vitest";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Pi provider presets", () => {
  it("uses distinct managed keys instead of shadowing Pi-native providers", () => {
    const keys = piProviderPresets.map((preset) => preset.providerKey);

    expect(new Set(keys).size).toBe(keys.length);
    expect(keys.every((key) => key.startsWith("cc-switch-"))).toBe(true);
  });

  it("only supplies configuration defaults, never a second gateway decision", () => {
    for (const preset of piProviderPresets) {
      expect("allowGateway" in preset).toBe(false);
      expect(preset.settingsConfig.baseUrl).toMatch(/^https?:\/\//);
      expect(preset.settingsConfig.api).not.toBe("");
      expect(preset.settingsConfig.models.length).toBeGreaterThan(0);
      for (const model of preset.settingsConfig.models) {
        expect(model.reasoning).toBe(true);
        expect(model.input?.[0]).toBe("text");
      }
    }
  });

  it("uses Anthropic roots that produce the pinned Pi request paths", () => {
    const requestUrls = Object.fromEntries(
      piProviderPresets
        .filter((preset) => preset.settingsConfig.api === "anthropic-messages")
        .map((preset) => {
          const base = new URL(preset.settingsConfig.baseUrl);
          const basePath = base.pathname.replace(/\/+$/, "");
          base.pathname = `${basePath}/v1/messages`;
          return [preset.name, base.toString()];
        }),
    );

    expect(requestUrls).toMatchObject({
      "Kimi For Coding": "https://api.kimi.com/coding/v1/messages",
      PackyCode: "https://www.packyapi.ai/v1/messages",
      AICodeMirror: "https://api.aicodemirror.ai/api/claudecode/v1/messages",
    });
  });
});
