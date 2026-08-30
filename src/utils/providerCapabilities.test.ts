import { describe, it, expect } from "vitest";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  providerNeedsRouting,
  resolveCodexOfficialIdentity,
  supportsOfficialProxyTakeover,
} from "@/utils/providerCapabilities";

function mkProvider(overrides: Partial<Provider> = {}): Provider {
  return { id: "p1", name: "Test", settingsConfig: {}, ...overrides };
}

// wire_api 取自 config.toml；chat_completions 需转换（需路由），responses 直连。
const codexConfig = (wireApi: "chat_completions" | "responses") =>
  `model_provider = "custom"\n\n[model_providers.custom]\nname = "X"\nbase_url = "https://x.example/v1"\nwire_api = "${wireApi}"\n`;

describe("providerNeedsRouting", () => {
  it("allows explicit Codex Official cards during takeover", () => {
    const native = mkProvider({
      id: "codex-official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    });
    const managed = mkProvider({
      id: "managed-account-card",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct-managed",
        },
      },
    });
    const unbound = mkProvider({
      id: "follow-login",
      category: "official",
      settingsConfig: {
        auth: {
          auth_mode: "chatgpt",
          tokens: { refresh_token: "legacy-live-only-token" },
        },
        config: "",
      },
    });
    const fixedManaged = mkProvider({
      id: "codex-official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
      meta: {
        providerType: "codex_oauth",
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct-fixed-managed",
        },
      },
    });

    expect(resolveCodexOfficialIdentity("codex", native)).toBe("native_login");
    expect(resolveCodexOfficialIdentity("codex", managed)).toBe(
      "managed_account",
    );
    expect(resolveCodexOfficialIdentity("codex", unbound)).toBe("native_login");
    expect(resolveCodexOfficialIdentity("codex", fixedManaged)).toBe(
      "managed_account",
    );
    expect(resolveCodexOfficialIdentity("claude", managed)).toBeNull();

    expect(supportsOfficialProxyTakeover("codex", native)).toBe(true);
    expect(supportsOfficialProxyTakeover("codex", managed)).toBe(true);
    expect(supportsOfficialProxyTakeover("codex", unbound)).toBe(true);
  });

  it("does not infer Codex login identity from a stale Official category", () => {
    const storedAuthKey = mkProvider({
      id: "legacy-api-key",
      category: "official",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-legacy" },
        config: "",
      },
    });
    const storedBearer = mkProvider({
      id: "legacy-bearer",
      category: "official",
      settingsConfig: {
        auth: {},
        config: 'experimental_bearer_token = "sk-legacy"',
      },
    });
    const explicitOpenAi = mkProvider({
      id: "official-openai",
      category: "official",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-official" },
        config: 'model_provider = "openai"',
      },
    });
    const grokOfficial = mkProvider({
      id: "grokbuild-official",
      category: "official",
      settingsConfig: { config: "" },
    });
    const unmarkedCustom = mkProvider({
      id: "custom-upstream",
      category: "official",
      settingsConfig: {
        auth: {},
        config:
          'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://example.com/v1"',
      },
    });
    const implicitCustom = mkProvider({
      id: "implicit-custom-upstream",
      category: "official",
      settingsConfig: {
        auth: {},
        config: 'model_provider = "ollama"',
      },
    });
    expect(resolveCodexOfficialIdentity("codex", storedAuthKey)).toBe(
      "api_key",
    );
    expect(supportsOfficialProxyTakeover("codex", storedAuthKey)).toBe(false);
    expect(resolveCodexOfficialIdentity("codex", storedBearer)).toBeNull();
    expect(resolveCodexOfficialIdentity("codex", explicitOpenAi)).toBe(
      "api_key",
    );
    expect(resolveCodexOfficialIdentity("codex", grokOfficial)).toBeNull();
    expect(resolveCodexOfficialIdentity("codex", unmarkedCustom)).toBeNull();
    expect(resolveCodexOfficialIdentity("codex", implicitCustom)).toBeNull();

    const unifiedSession = mkProvider({
      id: "unified-session",
      category: "official",
      settingsConfig: {
        auth: {},
        config:
          'model_provider = "custom"\n[model_providers.custom]\nname = "OpenAI"\nrequires_openai_auth = true\nsupports_websockets = true\nwire_api = "responses"',
      },
    });
    expect(resolveCodexOfficialIdentity("codex", unifiedSession)).toBe(
      "native_login",
    );
  });

  it("keeps category-less fixed and managed legacy cards recognizable", () => {
    const fixed = mkProvider({
      id: "codex-official",
      settingsConfig: { auth: {}, config: "" },
    });
    const fixedApiKey = mkProvider({
      id: "codex-official",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-legacy" },
        config: "",
      },
    });
    const fixedCustom = mkProvider({
      id: "codex-official",
      settingsConfig: {
        auth: {},
        config: 'model_provider = "custom"',
      },
    });
    const managed = mkProvider({
      id: "legacy-managed",
      settingsConfig: { auth: {}, config: null },
      meta: {
        authBinding: {
          source: "managed_account",
          authProvider: "codex_oauth",
          accountId: "acct-managed",
        },
      },
    });

    expect(resolveCodexOfficialIdentity("codex", fixed)).toBe("native_login");
    expect(resolveCodexOfficialIdentity("codex", managed)).toBe(
      "managed_account",
    );
    expect(resolveCodexOfficialIdentity("codex", fixedApiKey)).toBeNull();
    expect(resolveCodexOfficialIdentity("codex", fixedCustom)).toBeNull();
    expect(providerNeedsRouting("codex", fixed)).toBe(false);
    expect(providerNeedsRouting("codex", managed)).toBe(false);
  });

  it("官方供应商一律不需要路由（即便 providerType 是 OAuth）", () => {
    const apps: AppId[] = ["claude", "codex", "claude-desktop"];
    for (const app of apps) {
      expect(
        providerNeedsRouting(
          app,
          mkProvider({
            category: "official",
            meta: { providerType: "xai_oauth" },
          }),
        ),
      ).toBe(false);
    }
  });

  describe("托管 OAuth：providerType 权威，与 apiFormat 无关（P2）", () => {
    it("Claude 下 xai_oauth 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });

    it("Claude 下 codex_oauth 即便 apiFormat 被改成 anthropic 仍需路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "codex_oauth", apiFormat: "anthropic" },
          }),
        ),
      ).toBe(true);
    });

    it("Claude 下 codex_oauth 即便 apiFormat 缺省（旧数据）仍需路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { providerType: "codex_oauth" } }),
        ),
      ).toBe(true);
    });

    it("Claude 下 github_copilot 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "github_copilot", apiFormat: "openai_chat" },
          }),
        ),
      ).toBe(true);
    });

    it("Codex 下 xai_oauth 需要路由（原生 Responses 也要注入 token）", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });

    it("grokbuild 下 xai_oauth 需要路由", () => {
      expect(
        providerNeedsRouting(
          "grokbuild",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });
  });

  describe("Claude 非 OAuth 按格式判定", () => {
    it("anthropic 原生直连不需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "anthropic" } }),
        ),
      ).toBe(false);
    });

    it("apiFormat 缺省视为原生直连，不需要路由", () => {
      expect(providerNeedsRouting("claude", mkProvider({ meta: {} }))).toBe(
        false,
      );
    });

    it("openai_chat 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "openai_chat" } }),
        ),
      ).toBe(true);
    });

    it("openai_responses 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "openai_responses" } }),
        ),
      ).toBe(true);
    });
  });

  describe("Codex 非 OAuth 按格式判定（Responses 直连）", () => {
    it("原生 Responses 不需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "openai_responses" } }),
        ),
      ).toBe(false);
    });

    it("Chat 格式需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "openai_chat" } }),
        ),
      ).toBe(true);
    });

    it("Anthropic 格式需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "anthropic" } }),
        ),
      ).toBe(true);
    });

    it("config 里 wire_api=chat_completions 需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({
            settingsConfig: { config: codexConfig("chat_completions") },
          }),
        ),
      ).toBe(true);
    });

    it("config 里 wire_api=responses 不需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ settingsConfig: { config: codexConfig("responses") } }),
        ),
      ).toBe(false);
    });
  });

  describe("Claude Desktop 路由判定", () => {
    it("proxy 模式需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude-desktop",
          mkProvider({ meta: { claudeDesktopMode: "proxy" } }),
        ),
      ).toBe(true);
    });

    it.each(["github_copilot", "codex_oauth", "xai_oauth"])(
      "direct 模式的托管 OAuth %s 仍需要路由",
      (providerType) => {
        expect(
          providerNeedsRouting(
            "claude-desktop",
            mkProvider({
              meta: {
                providerType,
                claudeDesktopMode: "direct",
              },
            }),
          ),
        ).toBe(true);
      },
    );
  });
});
