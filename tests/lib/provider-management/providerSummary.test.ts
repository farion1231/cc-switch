import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  extractProviderSummary,
  maskSecret,
} from "@/lib/provider-management/providerSummary";

const makeProvider = (
  overrides: Partial<Provider> & { settingsConfig: Record<string, unknown> },
): Provider => ({
  id: overrides.id ?? "minimax-a",
  name: overrides.name ?? "Minimax API A",
  settingsConfig: overrides.settingsConfig,
  websiteUrl: overrides.websiteUrl,
  notes: overrides.notes,
  category: overrides.category ?? "aggregator",
  meta: overrides.meta ?? {
    providerType: "aggregator",
    apiFormat: "openai_chat",
  },
});

describe("providerSummary", () => {
  it("masks API keys without exposing the raw secret", () => {
    expect(maskSecret("sk-1234567890abcdef")).toBe("sk-123...cdef");
    expect(maskSecret("short")).toBe("sho...ort");
    expect(maskSecret("   ")).toBeUndefined();
  });

  it("extracts Claude base URL, key fingerprint, models, and safe search text", () => {
    const rawKey = "sk-1234567890abcdef";
    const summary = extractProviderSummary(
      makeProvider({
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.minimax.test/v1",
            ANTHROPIC_AUTH_TOKEN: rawKey,
            ANTHROPIC_DEFAULT_SONNET_MODEL: "minimax-2.5",
            ANTHROPIC_DEFAULT_OPUS_MODEL: "minimax-2.7",
          },
        },
      }),
      "claude",
    );

    expect(summary.baseUrl).toBe("https://api.minimax.test/v1");
    expect(summary.apiKeyFingerprint).toBe("sk-123...cdef");
    expect(summary.apiFormat).toBe("openai_chat");
    expect(summary.modelSummary).toContain("Sonnet -> minimax-2.5");
    expect(summary.modelSummary).toContain("Opus -> minimax-2.7");
    expect(summary.searchText).toContain("minimax-2.7");
    expect(summary.searchText).toContain("sk-123...cdef");
    expect(summary.searchText).not.toContain(rawKey);
  });

  it("extracts Codex TOML base URL and model", () => {
    const rawKey = "sk-codex1234567890";
    const summary = extractProviderSummary(
      makeProvider({
        id: "codex-minimax",
        name: "Codex Minimax",
        settingsConfig: {
          auth: {
            OPENAI_API_KEY: rawKey,
          },
          config: `
model_provider = "custom"
model = "minimax-agent"

[model_providers.custom]
base_url = "https://codex.minimax.test/v1"
`,
        },
        meta: {
          providerType: "aggregator",
          apiFormat: "openai_responses",
        },
      }),
      "codex",
    );

    expect(summary.baseUrl).toBe("https://codex.minimax.test/v1");
    expect(summary.apiKeyFingerprint).toBe("sk-cod...7890");
    expect(summary.modelSummary).toBe("minimax-agent");
    expect(summary.searchText).toContain("codex.minimax.test");
    expect(summary.searchText).not.toContain(rawKey);
  });

  it("summarizes non-mapping agent models as names only", () => {
    const openClawSummary = extractProviderSummary(
      makeProvider({
        id: "openclaw-minimax",
        name: "OpenClaw Minimax",
        settingsConfig: {
          models: [
            { id: "minimax/minimax-2.7", name: "Minimax 2.7" },
            { id: "moonshot/kimi-k2", name: "Kimi K2" },
          ],
        },
      }),
      "openclaw",
    );

    const openCodeSummary = extractProviderSummary(
      makeProvider({
        id: "opencode-minimax",
        name: "OpenCode Minimax",
        settingsConfig: {
          models: {
            "minimax/minimax-2.7": {},
            "moonshot/kimi-k2": {},
          },
        },
      }),
      "opencode",
    );

    expect(openClawSummary.modelSummary).toBe(
      "minimax/minimax-2.7, moonshot/kimi-k2",
    );
    expect(openCodeSummary.modelSummary).toBe(
      "minimax/minimax-2.7, moonshot/kimi-k2",
    );
    expect(openClawSummary.modelSummary).not.toContain("Model=");
    expect(openCodeSummary.modelSummary).not.toContain("Model=");
  });
});
