import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  getProxyRequirement,
  PROXY_REASON_KEYS,
} from "@/utils/providerRouting";

// Minimal Provider factory — only the fields read by getProxyRequirement
// matter; the rest are filled with harmless defaults.
const makeProvider = (overrides: Partial<Provider> = {}): Provider =>
  ({
    id: "test-id",
    name: "Test Provider",
    settingsConfig: {},
    ...overrides,
  }) as Provider;

describe("getProxyRequirement", () => {
  it("exposes stable i18n keys (not translated strings)", () => {
    // Documents the contract: `reason` is a stable i18n key, never a
    // localized message. Callers (badge / dialog) translate it.
    expect(PROXY_REASON_KEYS).toEqual({
      copilot: "notifications.proxyReasonCopilot",
      openAIChat: "notifications.proxyReasonOpenAIChat",
      openAIResponses: "notifications.proxyReasonOpenAIResponses",
      geminiNative: "notifications.proxyReasonGeminiNative",
      claudeDesktop: "notifications.proxyReasonClaudeDesktop",
      fullUrl: "notifications.proxyReasonFullUrl",
    });
  });

  describe("Codex chat wire protocol", () => {
    it("requires routing when TOML wire_api is chat", () => {
      const provider = makeProvider({
        settingsConfig: {
          config: [
            'model_provider = "deepseek"',
            "[model_providers.deepseek]",
            'wire_api = "chat"',
          ].join("\n"),
        },
      });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonOpenAIChat",
      });
    });

    it("requires routing when meta.apiFormat is openai_chat (codex)", () => {
      const provider = makeProvider({
        meta: { apiFormat: "openai_chat" },
      });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonOpenAIChat",
      });
    });

    it("does NOT require routing when codex TOML wire_api is responses", () => {
      const provider = makeProvider({
        settingsConfig: {
          config: [
            'model_provider = "openai"',
            "[model_providers.openai]",
            'wire_api = "responses"',
          ].join("\n"),
        },
      });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });

  describe("Claude non-anthropic interface formats", () => {
    it("requires routing for openai_chat", () => {
      const provider = makeProvider({
        meta: { apiFormat: "openai_chat" },
      });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonOpenAIChat",
      });
    });

    it("requires routing for openai_responses", () => {
      const provider = makeProvider({
        meta: { apiFormat: "openai_responses" },
      });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonOpenAIResponses",
      });
    });

    it("requires routing for gemini_native (proxy transforms anthropic↔gemini)", () => {
      const provider = makeProvider({
        meta: { apiFormat: "gemini_native" },
      });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonGeminiNative",
      });
    });

    it("does NOT require routing for plain anthropic Claude", () => {
      const provider = makeProvider({
        meta: { apiFormat: "anthropic" },
        settingsConfig: {
          env: { ANTHROPIC_AUTH_TOKEN: "sk-test" },
        },
      });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });

  describe("Copilot-as-Claude", () => {
    it("requires routing when providerType is github_copilot (claude)", () => {
      const provider = makeProvider({
        meta: { providerType: "github_copilot" },
      });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonCopilot",
      });
    });

    it("requires routing for Copilot-as-Claude via usage_script templateType", () => {
      const provider = makeProvider({
        meta: { usage_script: { templateType: "github_copilot" } },
      } as Partial<Provider>);
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonCopilot",
      });
    });

    it("does NOT require routing for github_copilot on non-claude app", () => {
      const provider = makeProvider({
        meta: { providerType: "github_copilot" },
      });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });

  describe("Full URL mode", () => {
    it("requires routing for claude with isFullUrl", () => {
      const provider = makeProvider({ meta: { isFullUrl: true } });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonFullUrl",
      });
    });

    it("requires routing for codex with isFullUrl", () => {
      const provider = makeProvider({ meta: { isFullUrl: true } });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonFullUrl",
      });
    });

    it("does NOT require routing for gemini with isFullUrl", () => {
      const provider = makeProvider({ meta: { isFullUrl: true } });
      const result = getProxyRequirement(provider, "gemini" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });

  describe("Claude Desktop proxy mode", () => {
    it("requires routing when claudeDesktopMode is proxy", () => {
      const provider = makeProvider({
        meta: { claudeDesktopMode: "proxy" },
      });
      const result = getProxyRequirement(provider, "claude-desktop" as AppId);
      expect(result).toEqual({
        required: true,
        reason: "notifications.proxyReasonClaudeDesktop",
      });
    });

    it("does NOT require routing when claudeDesktopMode is not proxy", () => {
      const provider = makeProvider({
        meta: { claudeDesktopMode: "direct" },
      });
      const result = getProxyRequirement(provider, "claude-desktop" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });

  describe("Official providers never need routing", () => {
    const apps: AppId[] = [
      "claude" as AppId,
      "codex" as AppId,
      "gemini" as AppId,
    ];

    for (const appId of apps) {
      it(`returns required:false for official provider on ${appId}`, () => {
        // Even with routing-triggering meta, official short-circuits.
        const provider = makeProvider({
          category: "official",
          meta: { apiFormat: "openai_chat", isFullUrl: true },
        });
        const result = getProxyRequirement(provider, appId);
        expect(result).toEqual({ required: false, reason: null });
      });
    }
  });

  describe("Empty / undefined config", () => {
    it("returns required:false for provider with no meta and empty config", () => {
      const provider = makeProvider({ settingsConfig: {} });
      const result = getProxyRequirement(provider, "claude" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });

    it("returns required:false for codex provider with empty config string", () => {
      const provider = makeProvider({ settingsConfig: { config: "" } });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });

    it("returns required:false for codex provider with undefined config", () => {
      const provider = makeProvider({ settingsConfig: {} });
      const result = getProxyRequirement(provider, "codex" as AppId);
      expect(result).toEqual({ required: false, reason: null });
    });
  });
});
