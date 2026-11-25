import { describe, expect, it } from "vitest";
import { providerPresets as claudePresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { getIconMetadata, hasIcon, isTileIcon } from "@/icons/extracted";

describe("UniRoute provider presets", () => {
  it("registers the UniRoute icon as a tile icon with metadata", () => {
    expect(hasIcon("uniroute")).toBe(true);
    expect(isTileIcon("uniroute")).toBe(true);
    expect(getIconMetadata("uniroute")?.displayName).toBe("UniRoute");
  });

  it("registers UniRoute across Claude, Codex, Gemini, OpenCode, OpenClaw, and Hermes", () => {
    const claude = claudePresets.find((p) => p.name === "UniRoute");
    expect(claude).toBeDefined();
    expect(claude?.category).toBe("aggregator");
    expect(claude?.icon).toBe("uniroute");
    const env = (claude?.settingsConfig as { env?: Record<string, string> })
      ?.env;
    expect(env?.ANTHROPIC_BASE_URL).toBe("https://api.uniroute.app");

    const codex = codexProviderPresets.find((p) => p.name === "UniRoute");
    expect(codex).toBeDefined();
    expect(codex?.config).toContain('base_url = "https://api.uniroute.app/v1"');

    const gemini = geminiProviderPresets.find((p) => p.name === "UniRoute");
    expect(gemini).toBeDefined();
    expect(gemini?.baseURL).toBe("https://api.uniroute.app");

    const opencode = opencodeProviderPresets.find((p) => p.name === "UniRoute");
    expect(opencode).toBeDefined();
    expect(opencode?.settingsConfig.options?.baseURL).toBe(
      "https://api.uniroute.app/v1",
    );

    const openclaw = openclawProviderPresets.find((p) => p.name === "UniRoute");
    expect(openclaw).toBeDefined();
    expect(openclaw?.settingsConfig.baseUrl).toBe("https://api.uniroute.app");

    const hermes = hermesProviderPresets.find((p) => p.name === "UniRoute");
    expect(hermes).toBeDefined();
    expect(hermes?.settingsConfig.base_url).toBe("https://api.uniroute.app");
  });
});
