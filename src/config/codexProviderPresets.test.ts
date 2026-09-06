import { describe, expect, it } from "vitest";
import {
  codexProviderPresets,
  generateThirdPartyConfig,
} from "./codexProviderPresets";

describe("codexProviderPresets managed OAuth snapshots", () => {
  // 托管 OAuth 卡无静态 key：requires_openai_auth = true 会被后端 keyless
  // 安全闸拒绝切换（provider.codex.config.official_auth_fallback）。后端
  // 写入层对存量卡会强制归一为 false，预设从源头就不能再带 true。
  it("OAuth presets never declare the auth.json fallback", () => {
    const oauthPresets = codexProviderPresets.filter(
      (preset) => preset.requiresOAuth,
    );
    expect(oauthPresets.length).toBeGreaterThan(0);
    for (const preset of oauthPresets) {
      expect(preset.config, preset.name).toContain(
        "requires_openai_auth = false",
      );
    }
  });

  it("key-based third-party template keeps the fallback flag by default", () => {
    expect(
      generateThirdPartyConfig("acme", "https://api.acme.dev/v1", "m1"),
    ).toContain("requires_openai_auth = true");
  });

  it("exposes GitHub Copilot as a keyless Codex provider", () => {
    const preset = codexProviderPresets.find(
      (item) => item.providerType === "github_copilot",
    );

    expect(preset).toMatchObject({
      name: "GitHub Copilot",
      apiFormat: "openai_chat",
      requiresOAuth: true,
      auth: {},
    });
    expect(preset?.config).toContain(
      'base_url = "https://api.githubcopilot.com"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).toContain("requires_openai_auth = false");
  });
});
