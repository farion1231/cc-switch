import { describe, it, expect } from "vitest";
import {
  detectBalanceProvider,
  injectBalanceUsageScript,
} from "./balanceProviders";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";

type TestProvider = {
  settingsConfig?: Record<string, any>;
  meta?: Record<string, any>;
};

function expectBalanceEnabled(provider: TestProvider) {
  expect(provider.meta?.usage_script).toMatchObject({
    enabled: true,
    templateType: "balance",
    code: "",
  });
}

describe("detectBalanceProvider (88API)", () => {
  it("matches the 88API hosts with or without /v1", () => {
    for (const url of [
      "https://api.88api.ai",
      "https://api.88api.ai/v1",
      "https://88api.ai/v1/",
    ]) {
      expect(detectBalanceProvider(url)?.id).toBe("88api");
    }
  });

  it("ignores lookalike hosts, paths and plain HTTP", () => {
    for (const url of [
      "https://88api.ai.example.com",
      "https://example.com/88api.ai",
      "https://not88api.ai",
      "http://api.88api.ai",
    ]) {
      expect(detectBalanceProvider(url)).toBeNull();
    }
  });
});

describe("injectBalanceUsageScript", () => {
  it("enables the balance query when the 88API preset is added to Claude Code", () => {
    const preset = providerPresets.find((p) => p.name === "88API");
    expect(preset).toBeDefined();
    expectBalanceEnabled(
      injectBalanceUsageScript("claude", {
        settingsConfig: preset!.settingsConfig as Record<string, any>,
      }),
    );
  });

  it("enables the balance query when the 88API preset is added to Codex", () => {
    const preset = codexProviderPresets.find((p) => p.name === "88API");
    expect(preset).toBeDefined();
    expectBalanceEnabled(
      injectBalanceUsageScript("codex", {
        settingsConfig: { auth: preset!.auth, config: preset!.config },
      }),
    );
  });

  it("enables the balance query when the 88API preset is added to Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find((p) => p.name === "88API");
    expect(preset).toBeDefined();
    expectBalanceEnabled(
      injectBalanceUsageScript("claude-desktop", {
        settingsConfig: { env: { ANTHROPIC_BASE_URL: preset!.baseUrl } },
      }),
    );
  });

  it("keeps a usage script the provider already has", () => {
    const existing = { enabled: true, templateType: "custom", code: "({})" };
    const provider = injectBalanceUsageScript("claude", {
      settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://api.88api.ai" } },
      meta: { usage_script: existing },
    });
    expect(provider.meta?.usage_script).toBe(existing);
  });

  it("leaves other balance providers for the user to enable", () => {
    const provider: TestProvider = injectBalanceUsageScript("claude", {
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic" },
      },
    });
    expect(provider.meta?.usage_script).toBeUndefined();
  });
});
