import { describe, expect, it } from "vitest";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";

describe("Gemini Provider Presets", () => {
  const vertexFast = geminiProviderPresets.find(
    (preset) => preset.name === "google-vertex-fast",
  );

  it("should include google-vertex-fast preset", () => {
    expect(vertexFast).toBeDefined();
  });

  it("google-vertex-fast should include editable GEMINI_API_KEY field", () => {
    const env = (vertexFast!.settingsConfig as any).env;
    expect(env).toHaveProperty("GEMINI_API_KEY", "");
  });

  it("google-vertex-fast should use gemini-3.1-pro-preview as default model", () => {
    const env = (vertexFast!.settingsConfig as any).env;
    expect(env.GEMINI_MODEL).toBe("gemini-3.1-pro-preview");
    expect(vertexFast!.model).toBe("gemini-3.1-pro-preview");
  });
});

