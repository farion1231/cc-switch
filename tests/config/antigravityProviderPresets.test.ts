import { describe, expect, it } from "vitest";
import { antigravityProviderPresets } from "@/config/antigravityProviderPresets";

describe("antigravityProviderPresets", () => {
  it("only exposes Google Official and uses the Antigravity icon", () => {
    expect(antigravityProviderPresets).toHaveLength(1);
    expect(antigravityProviderPresets[0]).toMatchObject({
      name: "Google Official",
      websiteUrl: "https://ai.google.dev/",
      apiKeyUrl: "https://aistudio.google.com/apikey",
      category: "official",
      icon: "antigravity",
      theme: {
        icon: "antigravity",
      },
    });
  });
});
