import { describe, expect, it } from "vitest";
import {
  codexProviderPresets,
  generateThirdPartyConfig,
} from "./codexProviderPresets";
import { getCodexCustomTemplate } from "./codexTemplates";

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
});

describe("codexProviderPresets removed upstream options", () => {
  // openai/codex#3212 移除了 disable_response_storage 配置项（客户端默认
  // 即不存储响应）；继续生成只会换来 Codex 的 ignored-setting 警告。
  it("never writes disable_response_storage", () => {
    expect(getCodexCustomTemplate().config).not.toContain(
      "disable_response_storage",
    );
    expect(
      generateThirdPartyConfig("acme", "https://api.acme.dev/v1", "m1"),
    ).not.toContain("disable_response_storage");
    for (const preset of codexProviderPresets) {
      expect(preset.config, preset.name).not.toContain(
        "disable_response_storage",
      );
    }
  });
});
