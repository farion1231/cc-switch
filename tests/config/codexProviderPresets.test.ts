import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";

describe("Codex provider presets", () => {
  it("uses ChatGPT auth for the first OpenAI official preset", () => {
    expect(codexProviderPresets[0]).toMatchObject({
      name: "OpenAI Official (ChatGPT)",
      codexAuthMode: "chatgpt",
    });
  });

  it("uses API key auth for at least one non-official preset", () => {
    const apiKeyPreset = codexProviderPresets.find(
      (preset) => !preset.isOfficial && preset.codexAuthMode === "apikey",
    );

    expect(apiKeyPreset).toBeDefined();
  });
});
