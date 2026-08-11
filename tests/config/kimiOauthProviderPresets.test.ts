/** @fileoverview Product-surface contract for the managed Kimi OAuth provider. */

import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import en from "@/i18n/locales/en.json";

describe("Kimi OAuth provider surface", () => {
  const claudePreset = providerPresets.find(
    (entry) => entry.name === "Kimi Code (OAuth)",
  );

  it("offers managed Kimi OAuth only for Claude Code", () => {
    expect(claudePreset).toMatchObject({
      providerType: "kimi_oauth",
      requiresOAuth: true,
      apiFormat: "anthropic",
    });
    expect(
      codexProviderPresets.some(
        (entry) => (entry.providerType as string | undefined) === "kimi_oauth",
      ),
    ).toBe(false);
    expect(
      claudeDesktopProviderPresets.some(
        (entry) => (entry.providerType as string | undefined) === "kimi_oauth",
      ),
    ).toBe(false);
  });

  it("keeps ordinary Kimi API-key presets available", () => {
    expect(
      providerPresets.some((entry) => entry.name === "Kimi For Coding"),
    ).toBe(true);
    expect(
      codexProviderPresets.some((entry) => entry.name === "Kimi For Coding"),
    ).toBe(true);
    expect(
      claudeDesktopProviderPresets.some(
        (entry) => entry.name === "Kimi For Coding",
      ),
    ).toBe(true);
  });

  it("uses the supported K3 role defaults and 256K context window", () => {
    const env = (
      claudePreset!.settingsConfig as { env: Record<string, string> }
    ).env;
    expect(env).toMatchObject({
      ANTHROPIC_BASE_URL: "https://api.kimi.com/coding/",
      ANTHROPIC_MODEL: "k3",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "k3-256k",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "k3",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "k3",
      CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
      CLAUDE_CODE_AUTO_COMPACT_WINDOW: "262144",
    });
  });

  it("preserves the approved OAuth Authentication Center wording", () => {
    expect(en.settings.authCenter).toMatchObject({
      title: "OAuth Authentication Center",
      description:
        "Use your other subscriptions in Claude Code — please be mindful of compliance risks.",
    });
  });
});
