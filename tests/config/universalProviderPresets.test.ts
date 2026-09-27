import { describe, expect, it } from "vitest";
import {
  createUniversalProviderFromPreset,
  findPresetByType,
  normalizeUniversalCodexBaseUrl,
  normalizeUniversalHermesBaseUrl,
} from "@/config/universalProviderPresets";

describe("CodeBuddy universal provider preset", () => {
  it("targets Claude and Codex through the Chat Completions router", () => {
    const preset = findPresetByType("codebuddy");

    expect(preset).toBeDefined();
    expect(preset?.defaultBaseUrl).toBe(
      "https://copilot.tencent.com/v2/chat/completions",
    );
    expect(preset?.defaultApps).toEqual({
      claude: true,
      codex: true,
      gemini: false,
      hermes: true,
    });
    expect(preset?.defaultModels.claude?.model).toBe("deepseek-v4-flash");
    expect(preset?.defaultModels.codex?.model).toBe("deepseek-v4-flash");
    expect(preset?.defaultModels.hermes?.model).toBe("deepseek-v4-flash");
    expect(preset?.meta?.apiFormat).toBe("openai_chat");
    expect(preset?.meta?.isFullUrl).toBe(true);
  });

  it("copies routing metadata into the saved universal provider", () => {
    const preset = findPresetByType("codebuddy")!;
    const provider = createUniversalProviderFromPreset(
      preset,
      "codebuddy-cn",
      preset.defaultBaseUrl!,
      "secret",
    );

    expect(provider.meta).toEqual(preset.meta);
    expect(provider.meta).not.toBe(preset.meta);
  });

  it("keeps full endpoints and only appends /v1 to a bare origin", () => {
    expect(
      normalizeUniversalCodexBaseUrl(
        "https://copilot.tencent.com/v2/chat/completions",
      ),
    ).toBe("https://copilot.tencent.com/v2/chat/completions");
    expect(normalizeUniversalCodexBaseUrl("https://api.example.com")).toBe(
      "https://api.example.com/v1",
    );
  });

  it("removes the Chat endpoint suffix for Hermes base_url", () => {
    expect(
      normalizeUniversalHermesBaseUrl(
        "https://copilot.tencent.com/v2/chat/completions",
      ),
    ).toBe("https://copilot.tencent.com/v2");
    expect(normalizeUniversalHermesBaseUrl("https://api.example.com/v1")).toBe(
      "https://api.example.com/v1",
    );
  });
});
